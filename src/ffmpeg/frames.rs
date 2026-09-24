//! The frames that OCR reads and CLIP embeds. Ten spread over the duration of
//! the file, which issue 01 measured against scene detection and `fps=1` and
//! found best.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

use crate::ffmpeg::{self, Tool};

/// How many frames one file gives, at most.
const MAX_FRAMES: u32 = 10;
/// The rate a very short clip is held to, so a two second clip gives 8 frames
/// and not 10 copies of the same one.
const MAX_FPS: f64 = 4.0;

/// Extracted frames, in order. The directory goes with them.
pub struct Frames {
    dir: PathBuf,
    pub paths: Vec<PathBuf>,
}

impl Drop for Frames {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Write the frames of one file to a temporary directory. A still has no
/// duration and gives its single image. PNG, because every decoder reads it and
/// the recognizer must see what the file holds, not what a JPEG made of it.
pub fn sample(source: &Path, hash: &str, duration: Option<f64>) -> Result<Frames> {
    let dir = scratch_dir(hash);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    // Held from here, so a failure below still removes the directory.
    let mut frames = Frames {
        dir,
        paths: Vec::new(),
    };

    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .arg("-i")
        .arg(source)
        .args(["-map", "0:v:0", "-an", "-sn", "-dn"]);
    match rate(duration) {
        Some(fps) => command
            .args(["-vf", &format!("fps={fps:.6}")])
            .args(["-frames:v", &MAX_FRAMES.to_string()]),
        None => command.args(["-frames:v", "1"]),
    };
    command.arg(frames.dir.join("%02d.png"));
    ffmpeg::output(command).context("extract frames")?;

    let mut paths: Vec<PathBuf> = std::fs::read_dir(&frames.dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "png"))
        .collect();
    paths.sort();
    frames.paths = paths;
    Ok(frames)
}

/// One frame as raw `rgb24`, and its time in the file.
pub struct Pixels {
    pub time_ms: i64,
    pub rgb: Vec<u8>,
}

/// The same frames as `sample`, scaled so the short side is `size`, cut to a
/// square at the center, and read from a pipe.
pub fn pixels(source: &Path, duration: Option<f64>, size: u32) -> Result<Vec<Pixels>> {
    let square = format!(
        "scale={size}:{size}:force_original_aspect_ratio=increase:flags=bilinear,crop={size}:{size}"
    );
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .arg("-i")
        .arg(source)
        .args(["-map", "0:v:0", "-an", "-sn", "-dn"]);
    let fps = rate(duration);
    match fps {
        Some(fps) => command
            .args(["-vf", &format!("fps={fps:.6},{square}")])
            .args(["-frames:v", &MAX_FRAMES.to_string()]),
        None => command.args(["-vf", &square, "-frames:v", "1"]),
    };
    command.args(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]);
    let bytes = ffmpeg::output(command).context("extract frames")?;

    let frame_len = (size * size * 3) as usize;
    Ok(bytes
        .chunks_exact(frame_len)
        .enumerate()
        .map(|(i, rgb)| Pixels {
            // The `fps` filter puts frame n at n / fps.
            time_ms: fps.map_or(0, |fps| (i as f64 / fps * 1000.0).round() as i64),
            rgb: rgb.to_vec(),
        })
        .collect())
}

/// Where one call puts its frames. The hash names it, so a directory left
/// behind says which file it came from, but the hash alone is not enough: two
/// runs over the same bytes would share a directory, and the first to finish
/// would remove the frames of the second.
fn scratch_dir(hash: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let call = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "file-buddy-ocr-{hash}-{}-{call}",
        std::process::id()
    ))
}

/// The rate that spreads `MAX_FRAMES` over the whole file.
fn rate(duration: Option<f64>) -> Option<f64> {
    duration
        .filter(|d| *d > 0.0)
        .map(|d| (f64::from(MAX_FRAMES) / d).min(MAX_FPS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_frames_are_spread_over_the_duration() {
        assert_eq!(rate(Some(100.0)), Some(0.1));
        assert_eq!(rate(Some(20.0)), Some(0.5));
    }

    #[test]
    fn a_short_clip_is_held_to_the_rate_limit() {
        assert_eq!(rate(Some(1.0)), Some(MAX_FPS));
    }

    #[test]
    fn two_runs_over_the_same_bytes_get_their_own_directory() {
        assert_ne!(scratch_dir("abc"), scratch_dir("abc"));
    }

    #[test]
    fn a_still_has_no_rate() {
        assert_eq!(rate(None), None);
        assert_eq!(rate(Some(0.0)), None);
    }
}
