//! One folder of the roots, as the Folders page shows it: the folders in it,
//! its files, and why a file on disk is not in the search.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use globset::GlobSet;
use rusqlite::Connection;

use crate::db::files::{self, FileRow, State};
use crate::media::Kind;
use crate::search::{Hit, Signal};

#[derive(Debug, Clone)]
pub enum Entry {
    Folder(PathBuf),
    File(Hit),
    /// On disk, but no search finds it.
    Stray {
        path: PathBuf,
        why: Stray,
        size: u64,
        mtime: i64,
    },
}

/// The order of the files in a folder. Each group keeps its place: folders
/// first, then the indexed files, then the files the search cannot find.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Name,
    /// The newest first.
    Recent,
    /// The largest first.
    Size,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Name, Sort::Recent, Sort::Size];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => "Name",
            Sort::Recent => "Recent",
            Sort::Size => "Size",
        }
    }

    pub fn next(self) -> Sort {
        match self {
            Sort::Name => Sort::Recent,
            Sort::Recent => Sort::Size,
            Sort::Size => Sort::Name,
        }
    }

    /// Ties go by name, so equal files never change places.
    fn key(self, path: &Path, mtime: i64, size: u64) -> (Reverse<i64>, Reverse<u64>, String) {
        let name = sort_name(path);
        match self {
            Sort::Name => (Reverse(0), Reverse(0), name),
            Sort::Recent => (Reverse(mtime), Reverse(0), name),
            Sort::Size => (Reverse(0), Reverse(size), name),
        }
    }
}

/// Why a file on disk is not in the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stray {
    /// A media file the scan has not reached.
    Unindexed,
    Ignored,
    /// The extension is not one of the media kinds.
    Unmatched,
}

impl Stray {
    pub fn as_str(self) -> &'static str {
        match self {
            Stray::Unindexed => "not indexed yet",
            Stray::Ignored => "ignored",
            Stray::Unmatched => "not a media file",
        }
    }
}

impl Entry {
    pub fn path(&self) -> &Path {
        match self {
            Entry::Folder(path) | Entry::Stray { path, .. } => path,
            Entry::File(hit) => &hit.file.path,
        }
    }

    pub fn size(&self) -> Option<u64> {
        match self {
            Entry::Folder(_) => None,
            Entry::File(hit) => Some(hit.file.size),
            Entry::Stray { size, .. } => Some(*size),
        }
    }

    /// Seconds since the Unix epoch.
    pub fn mtime(&self) -> Option<i64> {
        match self {
            Entry::Folder(_) => None,
            Entry::File(hit) => Some(hit.file.mtime),
            Entry::Stray { mtime, .. } => Some(*mtime),
        }
    }

    pub fn hit(&self) -> Option<&Hit> {
        match self {
            Entry::File(hit) => Some(hit),
            _ => None,
        }
    }

    pub fn hit_mut(&mut self) -> Option<&mut Hit> {
        match self {
            Entry::File(hit) => Some(hit),
            _ => None,
        }
    }

    /// What stays the same while the index changes: the bytes of a file, which
    /// a rename keeps, or the path of anything else.
    pub fn key(&self) -> String {
        match self {
            Entry::File(hit) => hit.file.hash.clone(),
            other => other.path().to_string_lossy().into_owned(),
        }
    }
}

/// Everything in `dir`: folders first, then the files the search finds, then
/// the files it cannot find. A file that is not probed yet is in the index but
/// not whole, so it goes with the ones it cannot find. The chips limit the
/// indexed files only; the rest is there to explain what is missing.
///
/// `sort` orders the files in each group. Folders stay by name: the time of a
/// folder changes only when a file directly in it does, which is not what
/// "recent" means to a person.
pub fn list(
    conn: &Connection,
    dir: &Path,
    ignore: &GlobSet,
    kinds: &[Kind],
    sort: Sort,
) -> Result<Vec<Entry>> {
    let mut rows: HashMap<PathBuf, FileRow> = files::in_folder(conn, dir)?
        .into_iter()
        .map(|row| (row.path.clone(), row))
        .collect();

    let mut folders = Vec::new();
    let mut ready = Vec::new();
    let mut pending = Vec::new();
    let mut strays = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        // The scan does not follow links, so a link is not part of the roots.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            folders.push(path);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if let Some(file) = rows.remove(&path) {
            if !kinds.is_empty() && !kinds.contains(&file.kind) {
                continue;
            }
            if file.state == State::Pending.as_str() {
                pending.push(file);
            } else {
                ready.push(file);
            }
            continue;
        }
        let why = if ignore.is_match(&path) {
            Stray::Ignored
        } else if Kind::from_path(&path).is_none() {
            Stray::Unmatched
        } else {
            Stray::Unindexed
        };
        let metadata = entry.metadata().ok();
        let size = metadata.as_ref().map_or(0, |metadata| metadata.len());
        let mtime = metadata.as_ref().map_or(0, seconds);
        strays.push((why, path, size, mtime));
    }

    folders.sort_by_cached_key(|path| sort_name(path));
    let by_file = |file: &FileRow| sort.key(&file.path, file.mtime, file.size);
    ready.sort_by_cached_key(by_file);
    pending.sort_by_cached_key(by_file);
    strays.sort_by_cached_key(|(why, path, size, mtime)| (*why, sort.key(path, *mtime, *size)));

    let file = |file: FileRow| {
        Entry::File(Hit {
            file,
            score: 0.0,
            signal: Signal::Filename,
            matched: String::new(),
        })
    };
    Ok(folders
        .into_iter()
        .map(Entry::Folder)
        .chain(ready.into_iter().map(file))
        .chain(pending.into_iter().map(file))
        .chain(
            strays
                .into_iter()
                .map(|(why, path, size, mtime)| Entry::Stray {
                    path,
                    why,
                    size,
                    mtime,
                }),
        )
        .collect())
}

fn seconds(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs() as i64)
}

fn sort_name(path: &Path) -> String {
    FileRow::name_of(path).to_lowercase()
}
