//! The first stage. It reads what the file is, and so decides which stages come
//! after it.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::db::{files, fts};
use crate::ffmpeg::probe::{self, Probe};
use crate::index::jobs::{self, LONG_FILE_SECONDS, Stage};

pub fn run(conn: &Connection, hash: &str, source: &Path) -> Result<()> {
    let row = files::by_hash(conn, hash)?
        .into_iter()
        .next()
        .context("the file left the database")?;
    let probe = probe::probe(source, row.kind)?;
    files::save_probe(conn, hash, &probe)?;
    queue_later_stages(conn, hash, &probe)?;
    fts::refresh_hash(conn, hash)
}

/// What a file needs after the probe. A long file gets no expensive stage, but
/// keeps its filename in the index and appears in Review.
fn queue_later_stages(conn: &Connection, hash: &str, probe: &Probe) -> Result<()> {
    let long = probe.duration.is_some_and(|d| d > LONG_FILE_SECONDS);
    for stage in Stage::after_probe(probe.kind) {
        if long {
            jobs::skip(conn, hash, *stage, "longer than five minutes")?;
        } else if *stage == Stage::Whisper && probe.audio_codec.is_none() {
            jobs::skip(conn, hash, *stage, "no audio track")?;
        } else {
            jobs::enqueue(conn, hash, *stage)?;
        }
    }
    Ok(())
}
