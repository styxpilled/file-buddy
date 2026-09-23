//! The worker pool that drains the job queue. Each worker owns its own
//! connection, so a slow FFmpeg call never holds a lock on the database.
//!
//! What a stage produces is in `stages`. This file only decides when a stage
//! runs, how many run at once, and what a failure means.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, anyhow};

use crate::db::{Db, files};
use crate::index::jobs::{self, MAX_ATTEMPTS, Stage};
use crate::index::stages;
use crate::paths;

#[derive(Debug, Default)]
pub struct StageStats {
    pub done: usize,
    pub failed: usize,
}

#[derive(Debug, Default)]
pub struct PipelineStats {
    pub probe: StageStats,
    pub thumbnail: StageStats,
}

/// Run every stage that has a worker, in order. Probe first, because it decides
/// which later stages a file needs.
pub fn run(db_path: &Path, cache: &Path) -> Result<PipelineStats> {
    let db = Db::open(db_path)?;
    jobs::requeue_running(db.conn())?;
    drop(db);

    Ok(PipelineStats {
        probe: run_stage(db_path, cache, Stage::Probe)?,
        thumbnail: run_stage(db_path, cache, Stage::Thumbnail)?,
    })
}

/// Read the on-screen text of every file that waits for it. Minutes of work on
/// a first run where speech is hours, so it runs first and separately.
pub fn read_text(db_path: &Path, cache: &Path) -> Result<StageStats> {
    run_stage(db_path, cache, Stage::Ocr)
}

/// Transcribe every file that waits for it. Hours of work on a first run, so it
/// is separate from `run` and the window can show results before it starts.
/// Without the model it stops here, before a job can fail for that reason.
pub fn transcribe(db_path: &Path, cache: &Path) -> Result<StageStats> {
    paths::whisper_model()?;
    run_stage(db_path, cache, Stage::Whisper)
}

/// Drain one stage with as many workers as the stage allows.
fn run_stage(db_path: &Path, cache: &Path, stage: Stage) -> Result<StageStats> {
    let done = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let workers = stage
        .concurrency()
        .min(jobs::pending_count(Db::open(db_path)?.conn(), stage)? as usize);
    if workers == 0 {
        return Ok(StageStats::default());
    }

    let result: Result<()> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..workers {
            handles.push(scope.spawn(|| worker(db_path, cache, stage, &done, &failed)));
        }
        for handle in handles {
            handle
                .join()
                .map_err(|_| anyhow!("a {} worker panicked", stage.as_str()))??;
        }
        Ok(())
    });
    result?;

    Ok(StageStats {
        done: done.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
    })
}

fn worker(
    db_path: &Path,
    cache: &Path,
    stage: Stage,
    done: &AtomicUsize,
    failed: &AtomicUsize,
) -> Result<()> {
    let mut db = Db::open(db_path)?;
    loop {
        let claimed = jobs::claim(db.conn_mut(), stage, 1)?;
        let Some(job) = claimed.into_iter().next() else {
            return Ok(());
        };
        match stages::run(db.conn(), cache, &job) {
            Ok(()) => {
                jobs::finish(db.conn(), &job.hash, stage)?;
                done.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                let message = format!("{e:#}");
                log::warn!("{} failed for {}: {message}", stage.as_str(), job.hash);
                jobs::fail(db.conn(), &job.hash, stage, &message)?;
                // The last attempt. Record it on the file, so Review can show it
                // and a rescan does not try again.
                if job.attempts + 1 >= MAX_ATTEMPTS {
                    files::mark_failed(db.conn(), &job.hash, &message)?;
                    failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}
