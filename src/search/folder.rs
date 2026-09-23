//! One folder of the roots, as the Folders page shows it: the folders in it,
//! its files, and why a file on disk is not in the search.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    },
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
pub fn list(conn: &Connection, dir: &Path, ignore: &GlobSet, kinds: &[Kind]) -> Result<Vec<Entry>> {
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
        strays.push((why, path));
    }

    folders.sort_by_cached_key(|path| sort_name(path));
    ready.sort_by_cached_key(|file| sort_name(&file.path));
    pending.sort_by_cached_key(|file| sort_name(&file.path));
    strays.sort_by_cached_key(|(why, path)| (*why, sort_name(path)));

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
                .map(|(why, path)| Entry::Stray { path, why }),
        )
        .collect())
}

fn sort_name(path: &Path) -> String {
    FileRow::name_of(path).to_lowercase()
}
