//! Files from outside the roots, and forget. A drop on the window adds files
//! once, with no watch. Forget takes a file out of the index: a dropped file
//! leaves it, and a root file is scanned again at once, so every stage runs
//! again from the start.

use std::path::{Path, PathBuf};

use anyhow::Result;
use globset::GlobSet;
use walkdir::WalkDir;

use crate::db::{Db, files};
use crate::index::{scan, watch};
use crate::media::Kind;
use crate::paths;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Added {
    /// The new dropped files, in the order they were found.
    pub dropped: Vec<i64>,
    /// Every file that joined the index, root files with it.
    pub joined: usize,
    /// Files that were in the index already.
    pub known: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Forgot {
    /// Dropped files, and root files no longer on disk, now out of the index.
    pub forgotten: usize,
    /// Root files, back in the index with their stages queued again.
    pub rescanned: usize,
}

/// The path as the index writes it, and whether it is dropped. A path under a
/// root is a root file, written as the scan writes it.
pub fn place(path: &Path, roots: &[PathBuf]) -> (PathBuf, bool) {
    match watch::under_root(path, roots) {
        Some(path) => (path, false),
        None => (path.to_path_buf(), true),
    }
}

/// Add the dropped files, and the media files in and under each dropped folder.
pub fn add(
    db: &mut Db,
    paths: &[PathBuf],
    roots: &[PathBuf],
    ignore: &GlobSet,
    cache: &Path,
) -> Result<Added> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            found.extend(
                WalkDir::new(path)
                    .follow_links(false)
                    .sort_by_file_name()
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_file())
                    .map(walkdir::DirEntry::into_path),
            );
        } else {
            found.push(path.clone());
        }
    }

    let mut added = Added::default();
    let conn = db.conn();
    for path in found {
        let Some(kind) = Kind::from_path(&path) else {
            continue;
        };
        if ignore.is_match(&path) {
            continue;
        }
        let (path, dropped) = place(&path, roots);
        if files::is_present(conn, &path)? {
            added.known += 1;
            continue;
        }
        match scan::visit(conn, &path, kind, dropped, cache) {
            Ok((id, _)) => {
                added.joined += 1;
                if dropped {
                    added.dropped.push(id);
                }
            }
            Err(e) => log::warn!("drop: skipped {}: {e:#}", path.display()),
        }
    }
    Ok(added)
}

/// Forget each file. The tags stay with the bytes, and so do the rows the user
/// edited.
pub fn forget(db: &mut Db, rows: &[files::FileRow], cache: &Path) -> Result<Forgot> {
    let mut forgot = Forgot::default();
    let transaction = db.conn_mut().transaction()?;
    for row in rows {
        let dropped = files::is_dropped(&transaction, row.id)?;
        files::forget(&transaction, row.id)?;
        if dropped {
            forgot.forgotten += 1;
            continue;
        }
        files::clear_results(&transaction, &row.hash)?;
        for made in [
            paths::thumbnail_path(cache, &row.hash),
            paths::tile_path(cache, &row.hash),
            paths::proxy_path(cache, &row.hash),
        ] {
            std::fs::remove_file(made).ok();
        }
        match Kind::from_path(&row.path) {
            Some(kind) if row.path.is_file() => {
                scan::visit(&transaction, &row.path, kind, false, cache)?;
                forgot.rescanned += 1;
            }
            _ => forgot.forgotten += 1,
        }
    }
    transaction.commit()?;
    Ok(forgot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tags;
    use crate::index::jobs::{self, Stage};

    #[test]
    fn a_path_under_a_root_is_a_root_file() {
        let roots = [PathBuf::from(r"D:\clips")];
        assert_eq!(
            place(Path::new(r"D:\Clips\a\b.mp4"), &roots),
            (PathBuf::from(r"D:\clips\a\b.mp4"), false)
        );
        assert_eq!(
            place(Path::new(r"E:\other\b.mp4"), &roots),
            (PathBuf::from(r"E:\other\b.mp4"), true)
        );
    }

    fn present(db: &Db) -> Vec<(String, bool)> {
        db.conn()
            .prepare(
                "SELECT filename, dropped FROM files WHERE missing_at IS NULL ORDER BY filename",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn a_drop_adds_files_once_and_a_scan_keeps_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("deeper")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("r.mp4"), "root").unwrap();
        std::fs::write(outside.join("a.mp4"), "a").unwrap();
        std::fs::write(outside.join("deeper").join("b.png"), "b").unwrap();
        std::fs::write(outside.join("notes.txt"), "no").unwrap();
        let roots = vec![root.clone()];
        let ignore = GlobSet::empty();
        let cache = dir.path().join("cache");
        let mut db = Db::open_in_memory().unwrap();

        let added = add(
            &mut db,
            &[outside.clone(), root.join("r.mp4")],
            &roots,
            &ignore,
            &cache,
        )
        .unwrap();
        assert_eq!((added.dropped.len(), added.joined, added.known), (2, 3, 0));
        let again = add(&mut db, &[outside.join("a.mp4")], &roots, &ignore, &cache).unwrap();
        assert_eq!((again.joined, again.known), (0, 1));

        scan::scan(&mut db, &roots, &ignore, &cache).unwrap();
        assert_eq!(
            present(&db),
            [
                ("a.mp4".into(), true),
                ("b.png".into(), true),
                ("r.mp4".into(), false)
            ]
        );

        std::fs::remove_file(outside.join("a.mp4")).unwrap();
        scan::scan(&mut db, &roots, &ignore, &cache).unwrap();
        assert_eq!(
            present(&db),
            [("b.png".into(), true), ("r.mp4".into(), false)]
        );
    }

    #[test]
    fn forget_drops_a_dropped_file_and_scans_a_root_file_again() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("r.mp4"), "root").unwrap();
        std::fs::write(dir.path().join("d.mp4"), "dropped").unwrap();
        let roots = vec![root.clone()];
        let ignore = GlobSet::empty();
        let cache = dir.path().join("cache");
        let mut db = Db::open_in_memory().unwrap();
        scan::scan(&mut db, &roots, &ignore, &cache).unwrap();
        add(
            &mut db,
            &[dir.path().join("d.mp4")],
            &roots,
            &ignore,
            &cache,
        )
        .unwrap();

        let root_file = files::by_path(db.conn(), &root.join("r.mp4"))
            .unwrap()
            .unwrap();
        jobs::finish(db.conn(), &root_file.hash, Stage::Probe).unwrap();
        tags::attach(db.conn_mut(), std::slice::from_ref(&root_file.hash), "kept").unwrap();
        let rows = files::all(db.conn()).unwrap();

        let forgot = forget(&mut db, &rows, &cache).unwrap();
        assert_eq!(
            forgot,
            Forgot {
                forgotten: 1,
                rescanned: 1
            }
        );
        assert_eq!(present(&db), [("r.mp4".into(), false)]);
        assert!(dir.path().join("d.mp4").is_file());

        let probe: String = db
            .conn()
            .query_row(
                "SELECT state FROM jobs WHERE hash = ?1 AND stage = 'probe'",
                [&root_file.hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(probe, "pending");
        assert_eq!(tags::of_hash(db.conn(), &root_file.hash).unwrap(), ["kept"]);
    }
}
