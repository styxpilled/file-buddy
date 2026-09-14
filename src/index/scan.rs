//! The walk over the configured roots. It fills `files` and queues the cheap
//! stages. It never touches a file beyond reading both ends for the hash.

use std::path::{Path, PathBuf};

use anyhow::Result;
use globset::GlobSet;
use walkdir::WalkDir;

use crate::db::{Db, files, fts};
use crate::ffmpeg::thumbnail;
use crate::index::jobs::{self, Stage};
use crate::media::{Kind, hash};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanStats {
    pub added: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub removed: usize,
    /// Files that could not be read at all, so they are not in the database.
    pub unreadable: usize,
}

/// Walk every root, update the database and queue probe and thumbnail work.
pub fn scan(db: &mut Db, roots: &[PathBuf], ignore: &GlobSet, cache: &Path) -> Result<ScanStats> {
    let mut stats = ScanStats::default();
    let mut seen_ids = Vec::new();

    for root in roots {
        if !root.exists() {
            log::warn!("root {} does not exist", root.display());
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !entry.file_type().is_file() || ignore.is_match(path) {
                continue;
            }
            let Some(kind) = Kind::from_path(path) else {
                continue;
            };
            match visit(db, path, kind, cache) {
                Ok((id, outcome)) => {
                    seen_ids.push(id);
                    match outcome {
                        files::Upsert::Added => stats.added += 1,
                        files::Upsert::Changed => stats.changed += 1,
                        files::Upsert::Unchanged => stats.unchanged += 1,
                    }
                }
                Err(e) => {
                    log::warn!("skipped {}: {e:#}", path.display());
                    stats.unreadable += 1;
                }
            }
        }
    }

    stats.removed = files::remove_missing(db.conn_mut(), &seen_ids)?;
    Ok(stats)
}

fn visit(db: &Db, path: &Path, kind: Kind, cache: &Path) -> Result<(i64, files::Upsert)> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let hash = hash::file_hash(path, size)?;
    let conn = db.conn();
    let (id, outcome) = files::upsert(
        conn,
        &files::Scanned {
            path,
            hash: &hash,
            kind,
            size,
            mtime,
        },
    )?;

    // The filename changed or the row is new, so the search index needs the
    // current name. The other three columns come from the tables they live in.
    if outcome != files::Upsert::Unchanged || !in_index(db, id)? {
        fts::refresh(conn, id)?;
    }

    if outcome != files::Upsert::Unchanged {
        jobs::enqueue(conn, &hash, Stage::Probe)?;
    }
    // A moved thumbnail cache is the usual reason a known file needs this again.
    if !thumbnail::exists(&hash, cache) {
        jobs::enqueue(conn, &hash, Stage::Thumbnail)?;
    }
    Ok((id, outcome))
}

fn in_index(db: &Db, file_id: i64) -> Result<bool> {
    let count: i64 = db.conn().query_row(
        "SELECT count(*) FROM fts WHERE rowid = ?1",
        [file_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
