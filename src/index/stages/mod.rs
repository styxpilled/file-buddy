//! What each stage does to one file. `pipeline` decides when a stage runs and
//! what a failure means. These decide what the stage produces.
//!
//! A new stage is a new arm in `run`, usually with a file of its own here, and
//! a new variant in `jobs::Stage`.

mod clip;
mod ocr;
mod probe;
mod whisper;

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::db::edits::{self, Table};
use crate::db::files::{self, FileRow};
use crate::ffmpeg::thumbnail;
use crate::index::jobs::{Job, Stage};
use crate::worker::Worker;

/// The worker processes of one stage run. A process starts on its first job and
/// stops when this is dropped.
pub struct Workers {
    whisper: Mutex<Worker>,
    clip: Mutex<Worker>,
}

impl Default for Workers {
    fn default() -> Self {
        Self {
            whisper: Mutex::new(Worker::new(&["worker", "whisper"])),
            clip: Mutex::new(Worker::new(&["worker", "clip"])),
        }
    }
}

/// Run one claimed job. The error text ends up in the `jobs` row. A file with a
/// row the user edited keeps its rows, so the stage does not run for it.
pub fn run(conn: &Connection, cache: &Path, workers: &Workers, job: &Job) -> Result<()> {
    let edited = match job.stage {
        Stage::Whisper => Some(Table::Segments),
        Stage::Ocr => Some(Table::Ocr),
        _ => None,
    };
    if let Some(table) = edited
        && edits::any(conn, table, &job.hash)?
    {
        log::info!(
            "{} kept the edited rows of {}",
            job.stage.as_str(),
            job.hash
        );
        return Ok(());
    }
    let source = source(conn, &job.hash)?;
    match job.stage {
        Stage::Probe => probe::run(conn, &source),
        Stage::Thumbnail => thumbnail::generate(&source.path, &source.hash, cache),
        Stage::Whisper => whisper::run(conn, &source, &workers.whisper),
        Stage::Ocr => ocr::run(conn, &source),
        Stage::Clip => clip::run(conn, &source, &workers.clip),
    }
}

/// A file that still exists on disk and holds these bytes. Duplicates give more
/// than one candidate, and either one produces the same result.
fn source(conn: &Connection, hash: &str) -> Result<FileRow> {
    files::by_hash(conn, hash)?
        .into_iter()
        .find(|row| row.path.is_file())
        .with_context(|| format!("no readable file holds {hash}"))
}
