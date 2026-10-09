//! Animated WebP. FFmpeg 8.1 skips the animation chunks and decodes nothing, so
//! `image-webp` decodes the frames and FFmpeg encodes them into an H.264 proxy
//! in the cache. Every reader after the probe opens the proxy.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, anyhow, bail};
use image_webp::WebPDecoder;

use crate::ffmpeg::probe::Probe;
use crate::ffmpeg::{self, Tool};
use crate::media::Kind;
use crate::paths;

/// The `VP8X` flag that marks an animation.
const ANIMATION_FLAG: u8 = 0x02;
/// Browsers play a delay this short or shorter at `SHORT_DELAY_PLAYS_AT`.
const SHORT_DELAY_MS: u32 = 10;
const SHORT_DELAY_PLAYS_AT: u32 = 100;
/// The finest step the proxy keeps a delay to.
const MIN_TICK_MS: u32 = 10;
const QUALITY: &str = "12";

/// Whether the file is an animated WebP, from its header alone.
pub fn is_animated(path: &Path) -> Result<bool> {
    let mut header = [0u8; 21];
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    match file.read_exact(&mut header) {
        Ok(()) => Ok(animated_header(&header)),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn animated_header(header: &[u8; 21]) -> bool {
    &header[0..4] == b"RIFF"
        && &header[8..12] == b"WEBP"
        && &header[12..16] == b"VP8X"
        && header[20] & ANIMATION_FLAG != 0
}

/// The path FFmpeg should read for a file: its proxy, made now if it is missing,
/// or the file itself.
pub fn readable(path: &Path, hash: &str, cache: &Path) -> Result<PathBuf> {
    let proxy = paths::proxy_path(cache, hash);
    if proxy.is_file() {
        return Ok(proxy);
    }
    if is_animated(path)? {
        write_proxy(path, hash, cache)?;
        return Ok(proxy);
    }
    Ok(path.to_path_buf())
}

/// Decode every frame, write the proxy, and say what the file is.
pub fn write_proxy(path: &Path, hash: &str, cache: &Path) -> Result<Probe> {
    let decoder = open(path)?;
    let (width, height) = decoder.dimensions();
    if !decoder.is_animated() {
        bail!("not an animated WebP");
    }
    let delays = read_delays(path)?;
    let tick = tick_ms(&delays);
    let repeats = repeats(&delays, tick);

    let destination = paths::proxy_path(cache, hash);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let partial = destination.with_extension("part.mp4");
    let written = encode(decoder, &repeats, tick, &partial);
    if let Err(e) = written {
        std::fs::remove_file(&partial).ok();
        return Err(e.context("write the proxy"));
    }
    std::fs::rename(&partial, &destination)
        .with_context(|| format!("move the proxy to {}", destination.display()))?;

    let total_ms: u32 = delays.iter().sum();
    Ok(Probe {
        kind: Kind::Animated,
        duration: Some(f64::from(total_ms) / 1000.0),
        width: Some(i64::from(width)),
        height: Some(i64::from(height)),
        video_codec: Some("webp".into()),
        audio_codec: None,
    })
}

fn open(path: &Path) -> Result<WebPDecoder<BufReader<File>>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    WebPDecoder::new(BufReader::new(file)).context("decode the WebP")
}

/// The delay of every frame, as a browser plays it. A frame is decoded to learn
/// its delay, so this is a pass of its own before the one that encodes.
fn read_delays(path: &Path) -> Result<Vec<u32>> {
    let mut decoder = open(path)?;
    let mut frame = vec![0; frame_len(&decoder)?];
    let mut delays = Vec::with_capacity(decoder.num_frames() as usize);
    for _ in 0..decoder.num_frames() {
        let delay = decoder.read_frame(&mut frame).context("decode a frame")?;
        delays.push(if delay <= SHORT_DELAY_MS {
            SHORT_DELAY_PLAYS_AT
        } else {
            delay
        });
    }
    if delays.is_empty() {
        bail!("the animation has no frames");
    }
    Ok(delays)
}

fn frame_len(decoder: &WebPDecoder<BufReader<File>>) -> Result<usize> {
    decoder
        .output_buffer_size()
        .ok_or_else(|| anyhow!("the frames are too large"))
}

/// The proxy runs at one frame per tick. The largest step that divides every
/// delay keeps them exact, and few frames repeat. Below `MIN_TICK_MS`, delays
/// round to it.
fn tick_ms(delays: &[u32]) -> u32 {
    delays.iter().copied().fold(0, gcd).max(MIN_TICK_MS)
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// How many ticks each frame shows for. The ends are rounded from the running
/// total, so rounding never adds up to drift.
fn repeats(delays: &[u32], tick: u32) -> Vec<u32> {
    let mut start_ms = 0u64;
    delays
        .iter()
        .map(|delay| {
            let end_ms = start_ms + u64::from(*delay);
            let ticks = (end_ms * 2 / u64::from(tick)).div_ceil(2)
                - (start_ms * 2 / u64::from(tick)).div_ceil(2);
            start_ms = end_ms;
            ticks.max(1) as u32
        })
        .collect()
}

/// Pipe the frames to FFmpeg as raw RGB. Transparent pixels go on black. The
/// size is padded to even numbers, which 4:2:0 needs.
fn encode(
    mut decoder: WebPDecoder<BufReader<File>>,
    repeats: &[u32],
    tick: u32,
    destination: &Path,
) -> Result<()> {
    let (width, height) = decoder.dimensions();
    let alpha = decoder.has_alpha();
    let mut frame = vec![0; frame_len(&decoder)?];

    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24"])
        .args(["-s", &format!("{width}x{height}")])
        .args(["-framerate", &format!("1000/{tick}")])
        .args(["-i", "pipe:0"])
        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", QUALITY])
        .args(["-pix_fmt", "yuv420p"])
        .arg(destination)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context("run FFmpeg")?;
    let mut stdin = child.stdin.take().context("open the FFmpeg input")?;

    // FFmpeg's error text is read while the frames go in, so a full pipe of it
    // cannot stall both sides.
    let mut stderr = child.stderr.take().context("open the FFmpeg errors")?;
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).ok();
        text
    });

    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    let sent: Result<()> = (|| {
        for count in repeats {
            decoder.read_frame(&mut frame).context("decode a frame")?;
            to_rgb(&frame, alpha, &mut rgb);
            for _ in 0..*count {
                stdin.write_all(&rgb)?;
            }
        }
        Ok(())
    })();
    drop(stdin);
    let status = child.wait().context("wait for FFmpeg")?;
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        let message = errors.lines().next_back().unwrap_or("no output").trim();
        bail!("{message}");
    }
    sent.context("send the frames to FFmpeg")
}

fn to_rgb(frame: &[u8], alpha: bool, rgb: &mut Vec<u8>) {
    rgb.clear();
    if !alpha {
        rgb.extend_from_slice(frame);
        return;
    }
    for pixel in frame.chunks_exact(4) {
        let a = u16::from(pixel[3]);
        rgb.extend(
            pixel[..3]
                .iter()
                .map(|c| ((u16::from(*c) * a + 127) / 255) as u8),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(chunk: &[u8; 4], flags: u8) -> [u8; 21] {
        let mut header = [0u8; 21];
        header[0..4].copy_from_slice(b"RIFF");
        header[8..12].copy_from_slice(b"WEBP");
        header[12..16].copy_from_slice(chunk);
        header[20] = flags;
        header
    }

    #[test]
    fn the_header_says_whether_a_webp_is_animated() {
        assert!(animated_header(&header(b"VP8X", 0x12)));
        assert!(!animated_header(&header(b"VP8X", 0x10)));
        assert!(!animated_header(&header(b"VP8 ", 0x02)));
        assert!(!animated_header(&header(b"VP8L", 0x02)));
        let mut png = header(b"VP8X", 0x02);
        png[0..4].copy_from_slice(b"\x89PNG");
        assert!(!animated_header(&png));
    }

    #[test]
    fn a_short_file_is_not_animated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.webp");
        std::fs::write(&path, b"RIFF").unwrap();
        assert!(!is_animated(&path).unwrap());
    }

    #[test]
    fn the_tick_divides_every_delay() {
        assert_eq!(tick_ms(&[40, 40, 80]), 40);
        assert_eq!(tick_ms(&[50, 70]), 10);
        assert_eq!(tick_ms(&[100]), 100);
    }

    #[test]
    fn a_tick_below_the_minimum_rounds_the_delays() {
        assert_eq!(tick_ms(&[33, 34, 33]), MIN_TICK_MS);
        let repeats = repeats(&[33, 34, 33], MIN_TICK_MS);
        assert_eq!(repeats.iter().sum::<u32>(), 10);
        assert_eq!(repeats, vec![3, 4, 3]);
    }

    #[test]
    fn rounding_does_not_drift() {
        let delays = vec![15; 100];
        let repeats = repeats(&delays, MIN_TICK_MS);
        assert_eq!(repeats.iter().sum::<u32>(), 150);
    }

    #[test]
    fn exact_delays_repeat_exactly() {
        assert_eq!(repeats(&[40, 80, 120], 40), vec![1, 2, 3]);
    }

    #[test]
    fn transparent_pixels_go_on_black() {
        let mut rgb = Vec::new();
        to_rgb(
            &[200, 100, 50, 255, 200, 100, 50, 0, 200, 100, 50, 128],
            true,
            &mut rgb,
        );
        assert_eq!(rgb, vec![200, 100, 50, 0, 0, 0, 100, 50, 25]);
    }
}
