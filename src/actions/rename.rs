//! Renaming a file on disk and in the index. The typed name replaces the stem
//! and the extension stays. A file that already holds the new name is never
//! replaced.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use rusqlite::Connection;
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND,
    ERROR_SHARING_VIOLATION, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
use windows::core::PCWSTR;

use crate::db::files::{self, FileRow};
use crate::media::filename;

/// The part of the name the user edits.
pub fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Give `file` the typed stem. `None` means the name did not change.
pub fn rename(conn: &mut Connection, file: &FileRow, typed: &str) -> Result<Option<PathBuf>> {
    let stem = filename::sanitize(typed).map_err(|e| anyhow!(e))?;
    let name = match file.path.extension() {
        Some(extension) => format!("{stem}.{}", extension.to_string_lossy()),
        None => stem,
    };
    if name == file.filename {
        return Ok(None);
    }
    let target = file.path.with_file_name(&name);
    // Windows names ignore case, so a change of case finds the file itself.
    let case_only = name.to_lowercase() == file.filename.to_lowercase();
    if target.exists() && !case_only {
        return Err(exists(&name));
    }

    // The row changes inside a transaction, so a move that fails leaves the
    // index as it was.
    let transaction = conn.transaction()?;
    files::set_path(&transaction, file.id, &target)?;
    move_no_replace(&file.path, &target).map_err(|e| match code(&e) {
        ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS => exists(&name),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => anyhow!("the file is gone"),
        ERROR_SHARING_VIOLATION => anyhow::Error::new(e).context("the file is in use"),
        _ => anyhow::Error::new(e).context("rename failed"),
    })?;
    if let Err(e) = transaction.commit() {
        // Disk and index must agree, so the file goes back.
        move_no_replace(&target, &file.path).ok();
        return Err(e.into());
    }
    Ok(Some(target))
}

/// Another process holds the file open without delete sharing. FFmpeg does,
/// so this is usually a decoder that has not closed yet.
pub fn is_in_use(e: &anyhow::Error) -> bool {
    e.root_cause()
        .downcast_ref::<io::Error>()
        .is_some_and(|e| code(e) == ERROR_SHARING_VIOLATION)
}

fn exists(name: &str) -> anyhow::Error {
    anyhow!("a file named {name} already exists")
}

fn code(e: &io::Error) -> WIN32_ERROR {
    WIN32_ERROR(e.raw_os_error().unwrap_or_default() as u32)
}

/// `std::fs::rename` replaces an existing target on Windows. Without
/// `MOVEFILE_REPLACE_EXISTING`, `MoveFileExW` refuses it instead.
fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from = wide(from);
    let to = wide(to);
    let moved = unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    };
    // The error of the `windows` crate holds the same code, but the rest of
    // this file reads an `io::Error`, which is what `is_in_use` downcasts to.
    moved.map_err(|_| io::Error::last_os_error())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use std::os::windows::fs::OpenOptionsExt;

    use super::*;
    use crate::db::{Db, fts};
    use crate::media::Kind;

    fn indexed(db: &Db, path: &Path) -> FileRow {
        std::fs::write(path, b"bytes").unwrap();
        let (id, _) = files::upsert(
            db.conn(),
            &files::Scanned {
                path,
                hash: "hash",
                kind: Kind::Video,
                size: 5,
                mtime: 0,
            },
        )
        .unwrap();
        fts::refresh(db.conn(), id).unwrap();
        files::by_path(db.conn(), path).unwrap().unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn renames_the_file_the_row_and_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_in_memory().unwrap();
        let file = indexed(&db, &dir.path().join("VID_20240914.mp4"));

        let target = rename(db.conn_mut(), &file, "tony: therapy?")
            .unwrap()
            .unwrap();

        assert_eq!(names(dir.path()), ["tony therapy.mp4"]);
        let row = files::by_path(db.conn(), &target).unwrap().unwrap();
        assert_eq!(
            (row.id, row.filename.as_str()),
            (file.id, "tony therapy.mp4")
        );
        let found: i64 = db
            .conn()
            .query_row("SELECT rowid FROM fts WHERE fts MATCH 'therapy'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(found, file.id);
    }

    #[test]
    fn never_replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_in_memory().unwrap();
        let file = indexed(&db, &dir.path().join("a.mp4"));
        std::fs::write(dir.path().join("b.mp4"), b"other").unwrap();

        let error = rename(db.conn_mut(), &file, "b").unwrap_err();

        assert_eq!(error.to_string(), "a file named b.mp4 already exists");
        assert_eq!(names(dir.path()), ["a.mp4", "b.mp4"]);
        assert_eq!(std::fs::read(dir.path().join("b.mp4")).unwrap(), b"other");
        assert!(files::by_path(db.conn(), &file.path).unwrap().is_some());
    }

    #[test]
    fn a_change_of_case_is_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_in_memory().unwrap();
        let file = indexed(&db, &dir.path().join("clip.mp4"));

        rename(db.conn_mut(), &file, "Clip").unwrap().unwrap();

        assert_eq!(names(dir.path()), ["Clip.mp4"]);
    }

    #[test]
    fn the_same_name_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_in_memory().unwrap();
        let file = indexed(&db, &dir.path().join("clip.mp4"));

        assert_eq!(rename(db.conn_mut(), &file, " clip ").unwrap(), None);
    }

    #[test]
    fn a_file_held_open_is_in_use_and_keeps_its_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_in_memory().unwrap();
        let file = indexed(&db, &dir.path().join("clip.mp4"));
        // Read and write sharing only, as FFmpeg opens a file.
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(&file.path)
            .unwrap();

        let error = rename(db.conn_mut(), &file, "new").unwrap_err();

        assert!(is_in_use(&error));
        assert!(files::by_path(db.conn(), &file.path).unwrap().is_some());
    }
}
