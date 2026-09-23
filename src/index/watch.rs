//! The watcher over the roots, and the incremental scan it starts. A change on
//! disk reaches the index in seconds, so the manual rescan is only for what the
//! watcher misses.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use globset::GlobSet;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::db::{Db, files};
use crate::index::scan;
use crate::media::{Kind, hash};

/// How long the roots must be quiet before the batch goes out. A download
/// writes one file many times, and this holds it to one visit.
const QUIET: Duration = Duration::from_millis(1200);
/// How long one batch may grow. A copy of a whole folder never goes quiet, so
/// the batch goes out anyway and the next one follows it.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// What one batch changed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WatchStats {
    pub added: usize,
    pub changed: usize,
    /// Paths that a known file moved to. No stage runs again for these.
    pub moved: usize,
    pub missing: usize,
}

impl WatchStats {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The running watcher. Dropping it stops the thread and the watch.
pub struct Watcher {
    _inner: RecommendedWatcher,
    batches: Receiver<Vec<PathBuf>>,
}

impl Watcher {
    /// The paths that changed and have gone quiet, or `None` while nothing has.
    pub fn batch(&self) -> Option<Vec<PathBuf>> {
        self.batches.try_recv().ok()
    }
}

/// Watch every root recursively. Paths the ignore globs match never leave the
/// debounce thread.
pub fn watch(roots: &[PathBuf], ignore: &GlobSet) -> Result<Watcher> {
    let (events, incoming) = channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        if let Ok(event) = event {
            events.send(event).ok();
        }
    })
    .context("start the file watcher")?;

    let mut watched = 0;
    for root in roots {
        if !root.exists() {
            log::warn!("root {} does not exist, not watched", root.display());
            continue;
        }
        watcher
            .watch(root, RecursiveMode::Recursive)
            .with_context(|| format!("watch {}", root.display()))?;
        watched += 1;
    }
    if watched == 0 {
        anyhow::bail!("no root to watch");
    }

    let (batches, receiver) = channel();
    let roots = roots.to_vec();
    let ignore = ignore.clone();
    std::thread::Builder::new()
        .name("file-buddy watch".into())
        .spawn(move || debounce(incoming, batches, &roots, &ignore))
        .context("start the debounce thread")?;

    Ok(Watcher {
        _inner: watcher,
        batches: receiver,
    })
}

/// Collect events until the roots are quiet, then send the paths as one batch.
/// It ends when the watcher is dropped and the channel closes.
fn debounce(
    incoming: Receiver<Event>,
    batches: Sender<Vec<PathBuf>>,
    roots: &[PathBuf],
    ignore: &GlobSet,
) {
    let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
    let mut opened_at = Instant::now();
    loop {
        match incoming.recv_timeout(QUIET) {
            Ok(event) => {
                if paths.is_empty() {
                    opened_at = Instant::now();
                }
                for path in interesting(&event, roots, ignore) {
                    paths.insert(path);
                }
                if opened_at.elapsed() < MAX_WAIT {
                    continue;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if !paths.is_empty()
            && batches
                .send(std::mem::take(&mut paths).into_iter().collect())
                .is_err()
        {
            return;
        }
    }
}

/// The paths of one event that the index cares about, written the way the scan
/// writes them.
fn interesting(event: &Event, roots: &[PathBuf], ignore: &GlobSet) -> Vec<PathBuf> {
    if matches!(event.kind, EventKind::Access(_)) {
        return Vec::new();
    }
    event
        .paths
        .iter()
        .filter_map(|path| under_root(path, roots))
        .filter(|path| !ignore.is_match(path))
        // A path with no media extension is only interesting when it is gone,
        // because a deleted folder is one event for the folder itself.
        .filter(|path| Kind::from_path(path).is_some() || !path.exists())
        .collect()
}

/// The path under the root that holds it, joined to the root as the
/// configuration writes it. The watcher reports `D:\clips\a.mp4` where a root
/// of `D:/clips` puts `D:/clips\a.mp4` in the database.
fn under_root(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let parts = comparable(path);
    let rest_of = |root: &Path, from: usize| -> PathBuf {
        let rest: PathBuf = path
            .components()
            .filter(|part| !matches!(part, Component::CurDir))
            .skip(from)
            .collect();
        if rest.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(rest)
        }
    };
    roots
        .iter()
        .map(|root| (root, comparable(root)))
        .find(|(_, root_parts)| parts.starts_with(root_parts))
        .map(|(root, root_parts)| rest_of(root, root_parts.len()))
}

/// The parts of a path, lowercased, so two spellings of one path compare equal.
fn comparable(path: &Path) -> Vec<String> {
    path.components()
        .filter(|part| !matches!(part, Component::CurDir))
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

/// Bring one batch of paths up to date. Paths that cannot be read yet come back
/// as the second value, because a file that is still being written reads short.
pub fn apply(
    db: &mut Db,
    paths: &[PathBuf],
    ignore: &GlobSet,
    cache: &Path,
) -> Result<(WatchStats, Vec<PathBuf>)> {
    let mut stats = WatchStats::default();
    let mut retry = Vec::new();

    // The paths that are gone first, so a file that moved can take the row of
    // the path it came from instead of a row of its own.
    let mut gone = Vec::new();
    let mut present = Vec::new();
    for path in paths {
        if ignore.is_match(path) {
            continue;
        }
        if !path.is_file() {
            gone.extend(files::rows_under(db.conn(), path)?);
        } else if let Some(kind) = Kind::from_path(path) {
            present.push((path.clone(), kind));
        }
    }

    for (path, kind) in present {
        let known = files::by_path(db.conn(), &path)?.is_some();
        if !known && let Some(at) = moved_from(&mut gone, &path) {
            let row = gone.remove(at);
            files::set_path(db.conn(), row.id, &path)?;
            stats.moved += 1;
        }
        match scan::visit(db.conn(), &path, kind, cache) {
            Ok((_, files::Upsert::Added)) => stats.added += 1,
            Ok((_, files::Upsert::Changed)) => stats.changed += 1,
            Ok((_, files::Upsert::Unchanged)) => {}
            Err(e) => {
                log::warn!("watch: {} is not readable yet: {e:#}", path.display());
                retry.push(path);
            }
        }
    }

    for row in gone {
        files::mark_gone(db.conn(), row.id)?;
        stats.missing += 1;
    }
    Ok((stats, retry))
}

/// The row of the path this file was moved from, matched by the bytes. A file
/// that is still being written hashes to something else, so it is not a move
/// and the normal path adds it.
fn moved_from(gone: &mut [files::FileRow], path: &Path) -> Option<usize> {
    if gone.is_empty() {
        return None;
    }
    let size = std::fs::metadata(path).ok()?.len();
    let hash = hash::file_hash(path, size).ok()?;
    gone.iter().position(|row| row.hash == hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_path_is_written_the_way_the_root_is() {
        let roots = [PathBuf::from("D:/clips")];
        assert_eq!(
            under_root(Path::new(r"D:\Clips\a\b.mp4"), &roots),
            Some(PathBuf::from(r"D:/clips\a\b.mp4"))
        );
        assert_eq!(under_root(Path::new(r"D:\other\b.mp4"), &roots), None);
    }

    #[test]
    fn the_root_itself_is_under_the_root() {
        let roots = [PathBuf::from("D:/clips")];
        assert_eq!(
            under_root(Path::new(r"D:\clips"), &roots),
            Some(PathBuf::from("D:/clips"))
        );
    }
}
