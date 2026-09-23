//! What each stage does to one file. `pipeline` decides when a stage runs and
//! what a failure means. These decide what the stage produces.
//!
//! A new stage is a new arm in `run`, usually with a file of its own here, and
//! a new variant in `jobs::Stage`.

mod ocr;
mod probe;
mod whisper;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::db::files::{self, FileRow};
use crate::ffmpeg::thumbnail;
use crate::index::jobs::{Job, Stage};

/// Run one claimed job. The error text ends up in the `jobs` row.
pub fn run(conn: &Connection, cache: &Path, job: &Job) -> Result<()> {
    let source = source(conn, &job.hash)?;
    match job.stage {
        Stage::Probe => probe::run(conn, &source),
        Stage::Thumbnail => thumbnail::generate(&source.path, &source.hash, cache),
        Stage::Whisper => whisper::run(conn, &source),
        Stage::Ocr => ocr::run(conn, &source),
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
