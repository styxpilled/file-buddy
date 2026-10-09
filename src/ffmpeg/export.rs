//! The Studio export: the FFmpeg arguments from an edit, and the run that
//! writes the file. The filters go in the order trim, crop, scale, text, speed.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::ffmpeg::{self, Tool};
use crate::studio::{Edit, Format, Quality};

/// H.264 needs even dimensions. This takes at most one pixel off an edge.
const EVEN: &str = "crop=trunc(iw/2)*2:trunc(ih/2)*2";
const AUDIO_BITRATE: &str = "192k";

/// The constant quality level of NVENC. Lower is better and larger.
fn nvenc_cq(quality: Quality) -> u8 {
    match quality {
        Quality::Best => 16,
        Quality::High => 21,
        Quality::Medium => 26,
        Quality::Low => 31,
    }
}

/// Everything after the shared options of `ffmpeg::command`. The trim is an
/// input option, so FFmpeg does not decode what comes before the in point.
pub fn args(edit: &Edit, format: Format, output: &Path) -> Vec<OsString> {
    let range = edit.range;
    let mut args = Args::default();
    args.push(&[
        "-ss",
        &seconds(range.start_ms),
        "-t",
        &seconds(range.len_ms()),
    ]);
    args.push(&["-i"]);
    args.0.push(edit.input.clone().into_os_string());
    args.push(&["-map", "0:v:0", "-map", "0:a:0?", "-sn", "-dn"]);
    args.push(&["-vf", EVEN]);
    match format {
        Format::Mp4 => {
            let cq = nvenc_cq(edit.quality).to_string();
            args.push(&["-c:v", "h264_nvenc", "-preset", "p5", "-tune", "hq"]);
            args.push(&["-rc", "vbr", "-cq", &cq, "-b:v", "0", "-pix_fmt", "yuv420p"]);
            args.push(&["-c:a", "aac", "-b:a", AUDIO_BITRATE]);
            args.push(&["-movflags", "+faststart", "-f", "mp4"]);
        }
    }
    args.0.push(output.as_os_str().to_owned());
    args.0
}

#[derive(Default)]
struct Args(Vec<OsString>);

impl Args {
    fn push(&mut self, items: &[&str]) {
        self.0.extend(items.iter().map(OsString::from));
    }
}

fn seconds(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// Write the export to `destination`. FFmpeg writes a partial file beside it,
/// which no scan takes for media, and the rename makes it whole. `progress`
/// gets the share of the range done, from 0 to 1.
pub fn run(
    edit: &Edit,
    format: Format,
    destination: &Path,
    mut progress: impl FnMut(f32),
) -> Result<()> {
    let partial = partial_path(destination);
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .args(["-progress", "pipe:1", "-nostats"])
        .args(args(edit, format, &partial))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context("run FFmpeg")?;

    // Read on its own thread, so a full pipe cannot stop FFmpeg.
    let stderr = child.stderr.take().context("no FFmpeg error pipe")?;
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        BufReader::new(stderr).read_to_string(&mut text).ok();
        text
    });

    let length_us = (edit.range.len_ms() * 1000).max(1) as f32;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(us) = line
                .strip_prefix("out_time_us=")
                .and_then(|value| value.trim().parse::<u64>().ok())
            {
                progress((us as f32 / length_us).clamp(0., 1.));
            }
        }
    }

    let status = child.wait().context("wait for FFmpeg")?;
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        std::fs::remove_file(&partial).ok();
        let errors = errors.trim();
        if errors.is_empty() {
            bail!("FFmpeg exited with {status}");
        }
        bail!("{errors}");
    }
    std::fs::rename(&partial, destination)
        .with_context(|| format!("move the export to {}", destination.display()))
}

fn partial_path(destination: &Path) -> PathBuf {
    let mut path = destination.as_os_str().to_owned();
    path.push(".part");
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Range;

    #[test]
    fn a_range_export_trims_the_input_and_encodes_h264_with_aac() {
        let mut edit = Edit::new(
            "D:/clips/cat jump.mkv".into(),
            "D:/cache/ab/abcd.proxy.mp4".into(),
            10_000,
        );
        edit.range = Range {
            start_ms: 3_250,
            end_ms: 7_000,
        };
        let args: Vec<String> = args(&edit, Format::Mp4, Path::new("D:/out/x.mp4.part"))
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "-ss",
                "3.250",
                "-t",
                "3.750",
                "-i",
                "D:/cache/ab/abcd.proxy.mp4",
                "-map",
                "0:v:0",
                "-map",
                "0:a:0?",
                "-sn",
                "-dn",
                "-vf",
                EVEN,
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p5",
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                "21",
                "-b:v",
                "0",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
                "D:/out/x.mp4.part",
            ]
        );
    }

    #[test]
    fn the_quality_sets_the_level() {
        let mut edit = Edit::new("a.mp4".into(), "a.mp4".into(), 1_000);
        edit.quality = Quality::Low;
        let args = args(&edit, Format::Mp4, Path::new("b.mp4"));
        let at = args.iter().position(|arg| arg == "-cq").unwrap();
        assert_eq!(args[at + 1], "31");
    }

    #[test]
    fn the_partial_file_is_not_media() {
        let partial = partial_path(Path::new("D:/out/cat 0m01-0m02.mp4"));
        assert_eq!(partial, Path::new("D:/out/cat 0m01-0m02.mp4.part"));
        assert_eq!(crate::media::Kind::from_path(&partial), None);
    }
}
