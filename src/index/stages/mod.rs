//! What each stage does to one file. `pipeline` decides when a stage runs and
//! what a failure means. These decide what the stage produces.
//!
//! A new stage is a new file here, a new arm in `run` and a new variant in
//! `jobs::Stage`.

mod ocr;
mod probe;
mod thumbnail;
mod whisper;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::db::files;
use crate::index::jobs::{Job, Stage};

/// Run one claimed job. The error text ends up in the `jobs` row.
pub fn run(conn: &Connection, cache: &Path, job: &Job) -> Result<()> {
    let source = source_path(conn, &job.hash)?;
    match job.stage {
        Stage::Probe => probe::run(conn, &job.hash, &source),
        Stage::Thumbnail => thumbnail::run(&job.hash, &source, cache),
        Stage::Whisper => whisper::run(conn, &job.hash, &source),
        Stage::Ocr => ocr::run(conn, &job.hash, &source),
    }
}

/// A file that still exists on disk and holds these bytes. Duplicates give more
/// than one candidate, and either one produces the same result.
fn source_path(conn: &Connection, hash: &str) -> Result<PathBuf> {
    let rows = files::by_hash(conn, hash)?;
    rows.iter()
        .map(|row| row.path.clone())
        .find(|path| path.is_file())
        .with_context(|| format!("no readable file holds {hash}"))
}
