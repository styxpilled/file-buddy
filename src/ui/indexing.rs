//! Keeping the index current while the window is open: the scan at the start
//! and on request, the watcher over the roots, and the text and speech stages
//! that follow both.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use globset::GlobSet;
use gpui::prelude::*;
use gpui::{Context, Task};

use crate::config::Paused;
use crate::db::Db;
use crate::index::jobs::{self, Stage};
use crate::index::watch::{self, WatchStats, Watcher};
use crate::index::{self, scan};
use crate::paths;
use crate::ui::{FileBuddy, Notice};

const PROGRESS_POLL: Duration = Duration::from_secs(2);
/// How often the window asks the watcher what changed. The batch it gets has
/// already waited for the roots to go quiet.
const WATCH_POLL: Duration = Duration::from_millis(500);
/// How often text that a stage found meanwhile reaches the results.
const INDEX_REFRESH: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct Indexer {
    /// A scan or a batch of changes is on its way into the database.
    scanning: bool,
    /// Settings saved new folders while a scan ran, so another scan follows it.
    rescan_after: bool,
    watcher: Option<Watcher>,
    /// What the watcher saw and the window has not indexed yet.
    pending: Vec<PathBuf>,
    /// Paths that were not readable last time. They get one more try.
    retried: HashSet<PathBuf>,
    /// The expensive stage that runs in the background, if one does.
    stage: Option<Stage>,
    /// A scan queued new work while a later stage ran, so the chain starts
    /// again from text when that stage ends.
    restart_after: bool,
    /// The workers of a stage read these before each claim.
    ocr_paused: Arc<AtomicBool>,
    clip_paused: Arc<AtomicBool>,
    whisper_paused: Arc<AtomicBool>,
    /// How far that stage is, or why it cannot run.
    pub progress: Option<Notice>,
}

impl Indexer {
    pub(super) fn new(paused: Paused) -> Self {
        Self {
            ocr_paused: Arc::new(AtomicBool::new(paused.ocr)),
            clip_paused: Arc::new(AtomicBool::new(paused.clip)),
            whisper_paused: Arc::new(AtomicBool::new(paused.whisper)),
            ..Self::default()
        }
    }

    fn paused(&self, stage: Stage) -> &Arc<AtomicBool> {
        match stage {
            Stage::Ocr => &self.ocr_paused,
            Stage::Clip => &self.clip_paused,
            _ => &self.whisper_paused,
        }
    }
}

impl FileBuddy {
    /// Hold a stage back, or let it go on. A job that runs finishes, and the
    /// rest wait in the queue.
    pub(super) fn pause_stage(&mut self, stage: Stage, paused: bool, cx: &mut Context<Self>) {
        match stage {
            Stage::Ocr => self.config.paused.ocr = paused,
            Stage::Clip => self.config.paused.clip = paused,
            _ => self.config.paused.whisper = paused,
        }
        self.indexer.paused(stage).store(paused, Ordering::Relaxed);
        if let Err(e) = paths::config_path().and_then(|path| self.config.write(&path)) {
            log::error!("saving the settings failed: {e:#}");
            self.flash(format!("saving failed: {e}"), true, cx);
        }
        if !paused {
            self.index_text(stage, cx);
        }
        cx.notify();
    }

    /// Bring the collection up to date, then show it. Without this the window
    /// shows what the last run indexed.
    pub(super) fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.indexer.scanning {
            self.indexer.rescan_after = true;
            return;
        }
        let roots = self.config.roots.clone();
        self.status = "scanning".into();
        self.update_index(
            move |db, ignore, cache| scan::scan(db, &roots, ignore, cache),
            |this, scanned, cx| {
                // The same query over a fresher index. The user may have
                // clicked a tile while the scan ran, so the selection stays.
                this.rerun_search(cx);
                match scanned {
                    Err(e) => this.status = format!("scan failed: {e:#}").into(),
                    Ok(_) => this.index_text(Stage::Ocr, cx),
                }
            },
            cx,
        );
    }

    /// Watch the roots and index what changes, so the collection stays current
    /// without a rescan.
    pub(super) fn watch_changes(&mut self, cx: &mut Context<Self>) {
        self.start_watching();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH_POLL).await;
                let alive = this.update(cx, |this, cx| {
                    let indexer = &mut this.indexer;
                    while let Some(batch) = indexer.watcher.as_ref().and_then(Watcher::batch) {
                        indexer.pending.extend(batch);
                    }
                    // A scan walks every root already, so a batch waits for it.
                    if !indexer.pending.is_empty() && !indexer.scanning {
                        let batch = std::mem::take(&mut indexer.pending);
                        this.apply_changes(batch, cx);
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// Start the watcher, or replace the one that watches the old folders.
    pub(super) fn start_watching(&mut self) {
        let Ok(ignore) = self.config.ignore_set() else {
            return;
        };
        self.indexer.watcher = watch::watch(&self.config.roots, &ignore)
            .inspect_err(|e| log::warn!("watching the folders failed: {e:#}"))
            .ok();
    }

    /// Index one batch of changed paths and show what it did.
    fn apply_changes(&mut self, mut paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        paths.sort();
        paths.dedup();
        self.update_index(
            move |db, ignore, cache| watch::apply(db, &paths, ignore, cache),
            |this, applied, cx| match applied {
                Err(e) => log::error!("indexing the changes failed: {e:#}"),
                Ok((stats, retry)) => {
                    this.requeue(retry);
                    if !stats.is_empty() {
                        this.rerun_search(cx);
                        this.select_pasted(cx);
                        this.flash(summary(&stats), false, cx);
                        this.index_text(Stage::Ocr, cx);
                    }
                }
            },
            cx,
        );
    }

    /// Run `walk` over the database in the background, then the cheap stages
    /// for what it queued, then `done` on the window.
    pub(super) fn update_index<T: Send + 'static>(
        &mut self,
        walk: impl FnOnce(&mut Db, &GlobSet, &Path) -> Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, Result<T>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(work) = self.index_work(walk, cx) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                done(this, result, cx);
                this.index_done(cx);
            })
            .ok();
        })
        .detach();
    }

    /// The background half of `update_index`, for a caller that needs the
    /// window when it ends. It calls `index_done` after.
    pub(super) fn index_work<T: Send + 'static>(
        &mut self,
        walk: impl FnOnce(&mut Db, &GlobSet, &Path) -> Result<T> + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<T>>> {
        let Ok(ignore) = self.config.ignore_set() else {
            self.status = "the ignore globs are not valid".into();
            return None;
        };
        let db_path = self.db_path.clone();
        let cache = self.cache.clone();
        self.indexer.scanning = true;
        Some(cx.background_spawn(async move {
            let value = walk(&mut Db::open(&db_path)?, &ignore, &cache)?;
            index::run(&db_path, &cache)?;
            Ok(value)
        }))
    }

    /// Two walks at once would visit the same files twice, so a scan asked for
    /// meanwhile waits for this one.
    pub(super) fn index_done(&mut self, cx: &mut Context<Self>) {
        self.indexer.scanning = false;
        if std::mem::take(&mut self.indexer.rescan_after) {
            self.rescan(cx);
        }
    }

    /// A path that was still being written comes back once. Unreadable twice
    /// and only a rescan brings it in.
    fn requeue(&mut self, retry: Vec<PathBuf>) {
        let indexer = &mut self.indexer;
        let (again, first): (Vec<PathBuf>, Vec<PathBuf>) = retry
            .into_iter()
            .partition(|path| indexer.retried.contains(path));
        for path in again {
            log::warn!("watch: gave up on {}", path.display());
        }
        indexer.retried = first.iter().cloned().collect();
        indexer.pending.extend(first);
    }

    /// Run one expensive stage in the background and show how far it is. Text
    /// comes first, then images, then speech, because the first two are
    /// minutes of work where speech is hours. The queue is durable, so a
    /// restart continues it, and a run that is already going picks up the jobs
    /// a later scan queues.
    pub(super) fn index_text(&mut self, stage: Stage, cx: &mut Context<Self>) {
        if let Some(running) = self.indexer.stage {
            self.indexer.restart_after |= stage == Stage::Ocr && running != Stage::Ocr;
            return;
        }
        if self.indexer.paused(stage).load(Ordering::Relaxed) {
            if let Some(next) = next_stage(stage) {
                self.index_text(next, cx);
            }
            return;
        }
        if let Some((model, file)) = match stage {
            Stage::Clip => Some((paths::clip_model(), paths::CLIP_MODEL)),
            Stage::Whisper => Some((paths::whisper_model(), paths::WHISPER_MODEL)),
            _ => None,
        } && let Err(e) = model
        {
            log::error!("{e:#}");
            self.indexer.progress = Some(Notice {
                text: format!(
                    "{} model missing: {}\\{file}",
                    stage_label(stage),
                    paths::MODEL_DIR
                )
                .into(),
                failed: true,
            });
            cx.notify();
            // Speech does not wait for images.
            if let Some(next) = next_stage(stage) {
                self.index_text(next, cx);
            }
            return;
        }

        self.indexer.stage = Some(stage);
        let db_path = self.db_path.clone();
        let cache = self.cache.clone();
        let paused = self.indexer.paused(stage).clone();
        let work = cx.background_spawn(async move {
            match stage {
                Stage::Ocr => index::read_text(&db_path, &cache, &paused),
                Stage::Clip => index::embed_frames(&db_path, &cache, &paused),
                _ => index::transcribe(&db_path, &cache, &paused),
            }
        });
        let progress = cx.spawn(async move |this, cx| {
            let mut refreshed_at = Instant::now();
            let mut refreshed_count = None;
            loop {
                let alive = this.update(cx, |this, cx| {
                    let Ok(progress) = jobs::progress(this.db.conn(), stage) else {
                        return;
                    };
                    if progress.finished < progress.total {
                        this.indexer.progress = Some(Notice {
                            text: format!(
                                "{} {} / {}",
                                stage_label(stage),
                                progress.finished,
                                progress.total
                            )
                            .into(),
                            failed: false,
                        });
                    }
                    // New text reaches the results now and then, not on every
                    // file, so the grid does not keep moving.
                    if refreshed_count != Some(progress.finished)
                        && refreshed_at.elapsed() >= INDEX_REFRESH
                    {
                        refreshed_at = Instant::now();
                        refreshed_count = Some(progress.finished);
                        this.rerun_search(cx);
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
                cx.background_executor().timer(PROGRESS_POLL).await;
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            drop(progress);
            this.update(cx, |this, cx| {
                this.indexer.stage = None;
                this.indexer.progress = result.err().map(|e| {
                    log::error!("{} failed: {e:#}", stage.as_str());
                    Notice {
                        text: format!("{} failed: {e:#}", stage_label(stage)).into(),
                        failed: true,
                    }
                });
                this.rerun_search(cx);
                if std::mem::take(&mut this.indexer.restart_after) {
                    this.index_text(Stage::Ocr, cx);
                } else if let Some(next) = next_stage(stage) {
                    this.index_text(next, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

/// What the header says one batch of changes did.
fn summary(stats: &WatchStats) -> String {
    [
        (stats.added, "new"),
        (stats.changed, "changed"),
        (stats.moved, "moved"),
        (stats.missing, "gone"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .collect::<Vec<_>>()
    .join(", ")
}

/// The expensive stage that runs after this one.
fn next_stage(stage: Stage) -> Option<Stage> {
    match stage {
        Stage::Ocr => Some(Stage::Clip),
        Stage::Clip => Some(Stage::Whisper),
        _ => None,
    }
}

/// What the header calls a stage while it runs.
fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::Ocr => "text",
        Stage::Clip => "clip",
        _ => "speech",
    }
}
