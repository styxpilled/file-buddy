//! The first audio track as the samples whisper reads: 16 kHz mono `f32le`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::ffmpeg::{self, Tool};

const SAMPLE_RATE: u32 = 16_000;

pub fn samples(source: &Path) -> Result<Vec<f32>> {
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .arg("-i")
        .arg(source)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-ac", "1"])
        .args(["-ar", &SAMPLE_RATE.to_string(), "-f", "f32le", "pipe:1"]);
    let bytes = ffmpeg::output(command).context("decode audio")?;
    Ok(bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}
