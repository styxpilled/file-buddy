//! What the frames show. The `clip` worker samples the frames through FFmpeg
//! and embeds them.

use std::sync::Mutex;

use anyhow::{Result, anyhow};
use rusqlite::Connection;

use crate::clip::Frame;
use crate::clip::image::Request;
use crate::db::files::FileRow;
use crate::db::vectors;
use crate::worker::Worker;

pub fn run(conn: &Connection, source: &FileRow, worker: &Mutex<Worker>) -> Result<()> {
    let request = Request {
        path: source.path.clone(),
        duration: source.duration,
    };
    let frames: Vec<Frame> = worker
        .lock()
        .map_err(|_| anyhow!("the clip worker lock is poisoned"))?
        .call(&request)?;
    vectors::replace(conn, &source.hash, &frames)
}
