//! What the frames show. The `clip` worker samples the frames through FFmpeg
//! and embeds them.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Result, anyhow};
use rusqlite::Connection;

use crate::clip::Frame;
use crate::clip::image::Request;
use crate::db::files::FileRow;
use crate::db::vectors;
use crate::worker::Worker;

/// `readable` is the file or its proxy. See `ffmpeg::webp`.
pub fn run(
    conn: &Connection,
    source: &FileRow,
    readable: &Path,
    worker: &Mutex<Worker>,
) -> Result<()> {
    let request = Request {
        path: readable.to_path_buf(),
        duration: source.duration,
    };
    let frames: Vec<Frame> = worker
        .lock()
        .map_err(|_| anyhow!("the clip worker lock is poisoned"))?
        .call(&request)?;
    vectors::replace(conn, &source.hash, &frames)
}
