//! One thumbnail per content hash, from the FFmpeg `thumbnail` filter, which
//! picks a representative frame and so avoids a black opening frame. Video,
//! animation and still image all take the same command.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::ffmpeg::{self, Tool};
use crate::paths;

/// The longest edge of a thumbnail, in pixels.
pub const MAX_EDGE: u32 = 480;
/// How many frames the filter compares before it picks one.
const CANDIDATES: u32 = 100;

/// Write the thumbnail of one file into the cache and return its path.
pub fn generate(source: &Path, hash: &str, cache: &Path) -> Result<PathBuf> {
    let destination = paths::thumbnail_path(cache, hash);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let filter = format!(
        "thumbnail=n={CANDIDATES},scale=w='min({MAX_EDGE},iw)':h='min({MAX_EDGE},ih)':\
         force_original_aspect_ratio=decrease:force_divisible_by=2:flags=lanczos"
    );
    let partial = destination.with_extension("part.jpg");
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .arg("-i")
        .arg(source)
        .args(["-map", "0:v:0", "-an", "-sn", "-dn"])
        .args(["-vf", &filter, "-frames:v", "1", "-q:v", "4"])
        .arg(&partial);
    if let Err(e) = ffmpeg::output(command) {
        std::fs::remove_file(&partial).ok();
        return Err(e.context("thumbnail"));
    }
    // FFmpeg can exit zero and write nothing when the filter gets no frame.
    if std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0) == 0 {
        std::fs::remove_file(&partial).ok();
        bail!("thumbnail failed: ffmpeg produced no frame");
    }
    // Renamed last, so a partial file is never mistaken for a finished one.
    std::fs::rename(&partial, &destination)
        .with_context(|| format!("move the thumbnail to {}", destination.display()))?;
    Ok(destination)
}

/// Whether the cache already holds this thumbnail.
pub fn exists(hash: &str, cache: &Path) -> bool {
    paths::thumbnail_path(cache, hash).is_file()
}
