//! The Studio export: the FFmpeg arguments from an edit, and the run that
//! writes the file. The filters go in the order trim, crop, scale, text, speed.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::ffmpeg::{self, Tool};
use crate::studio::{Controls, Crop, Edit, Format, Quality};

/// H.264 needs even dimensions. This takes at most one pixel off an edge.
const EVEN: &str = "crop=trunc(iw/2)*2:trunc(ih/2)*2";
const AAC_BITRATE: &str = "192k";
const OPUS_BITRATE: &str = "160k";

/// The constant quality level of `h264_nvenc`. Lower is better and larger.
fn h264_cq(quality: Quality) -> u8 {
    match quality {
        Quality::Best => 16,
        Quality::High => 21,
        Quality::Medium => 26,
        Quality::Low => 31,
        Quality::Lower => 36,
        Quality::Lowest => 41,
    }
}

/// The `av1_nvenc` level with the same SSIM as `h264_cq`, over 1:1 crops of
/// four real clips.
fn av1_cq(quality: Quality) -> u8 {
    match quality {
        Quality::Best => 18,
        Quality::High => 23,
        Quality::Medium => 30,
        Quality::Low => 38,
        Quality::Lower => 45,
        Quality::Lowest => 54,
    }
}

/// The `libwebp_anim` quality, from 0 to 100. Higher is better and larger.
fn webp_quality(quality: Quality) -> u8 {
    match quality {
        Quality::Best => 90,
        Quality::High => 80,
        Quality::Medium => 70,
        Quality::Low => 55,
        Quality::Lower => 45,
        Quality::Lowest => 35,
    }
}

/// Everything after the shared options of `ffmpeg::command`. The trim is an
/// input option, so FFmpeg does not decode what comes before the in point.
pub fn args(edit: &Edit, controls: &Controls, output: &Path) -> Vec<OsString> {
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
    args.push(&["-map", "0:v:0"]);
    match controls.format {
        Format::Mp4 | Format::Webm => args.push(&["-map", "0:a:0?"]),
        Format::Gif | Format::Webp => args.push(&["-an"]),
    }
    args.push(&["-sn", "-dn", "-vf", &video_filter(edit, controls)]);
    match controls.format {
        Format::Mp4 => {
            let cq = h264_cq(controls.mp4_quality).to_string();
            args.push(&["-c:v", "h264_nvenc", "-preset", "p5", "-tune", "hq"]);
            args.push(&["-rc", "vbr", "-cq", &cq, "-b:v", "0", "-pix_fmt", "yuv420p"]);
            args.push(&["-c:a", "aac", "-b:a", AAC_BITRATE]);
            args.push(&["-movflags", "+faststart", "-f", "mp4"]);
        }
        Format::Webm => {
            let cq = av1_cq(controls.webm_quality).to_string();
            args.push(&["-c:v", "av1_nvenc", "-preset", "p5", "-tune", "hq"]);
            args.push(&["-rc", "vbr", "-cq", &cq, "-b:v", "0", "-pix_fmt", "yuv420p"]);
            args.push(&["-c:a", "libopus", "-b:a", OPUS_BITRATE]);
            args.push(&["-f", "webm"]);
        }
        Format::Gif => args.push(&["-c:v", "gif", "-loop", "0", "-f", "gif"]),
        Format::Webp => {
            let quality = webp_quality(controls.webp_quality).to_string();
            args.push(&[
                "-c:v",
                "libwebp_anim",
                "-lossless",
                "0",
                "-quality",
                &quality,
            ]);
            args.push(&["-loop", "0", "-f", "webp"]);
        }
    }
    args.0.push(output.as_os_str().to_owned());
    args.0
}

/// A GIF makes its palette from its own frames, in the same FFmpeg call.
fn video_filter(edit: &Edit, controls: &Controls) -> String {
    let mut filter = frame_filter(edit);
    if let Some(fps) = controls.fps() {
        filter.push_str(&format!(",fps={fps}"));
    }
    if controls.format == Format::Gif {
        filter.push_str(",split[a][b];[a]palettegen[p];[b][p]paletteuse");
    }
    filter
}

/// The crop, then the scale. A frame with no crop only loses an odd pixel.
fn frame_filter(edit: &Edit) -> String {
    if edit.size.0 == 0 || edit.size.1 == 0 {
        return EVEN.to_string();
    }
    let crop = edit.crop;
    let (crop_width, crop_height) = (crop.width & !1, crop.height & !1);
    let mut filter = if crop == Crop::full(edit.size) {
        EVEN.to_string()
    } else {
        format!("crop={crop_width}:{crop_height}:{}:{}", crop.x, crop.y)
    };
    let (width, height) = edit.output_size();
    if (width, height) != (crop_width, crop_height) {
        filter.push_str(&format!(",scale={width}:{height}:flags=lanczos"));
    }
    filter
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
    controls: &Controls,
    destination: &Path,
    mut progress: impl FnMut(f32),
) -> Result<()> {
    let partial = partial_path(destination);
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .args(["-progress", "pipe:1", "-nostats"])
        .args(args(edit, controls, &partial))
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

    fn edit() -> Edit {
        let mut edit = Edit::new(
            "D:/clips/cat jump.mkv".into(),
            "D:/cache/ab/abcd.proxy.mp4".into(),
            10_000,
            (1920, 1080),
        );
        edit.range = Range {
            start_ms: 3_250,
            end_ms: 7_000,
        };
        edit
    }

    fn strings(controls: Controls) -> Vec<String> {
        strings_of(&edit(), controls)
    }

    fn strings_of(edit: &Edit, controls: Controls) -> Vec<String> {
        let output = format!("D:/out/x.{}.part", controls.format.extension());
        args(edit, &controls, Path::new(&output))
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect()
    }

    fn of(format: Format) -> Vec<String> {
        strings(Controls {
            format,
            ..Controls::default()
        })
    }

    /// The trim, the input and the video map, which every format shares.
    fn expected(rest: &[&str]) -> Vec<String> {
        [
            "-ss",
            "3.250",
            "-t",
            "3.750",
            "-i",
            "D:/cache/ab/abcd.proxy.mp4",
            "-map",
            "0:v:0",
        ]
        .iter()
        .chain(rest)
        .map(|arg| arg.to_string())
        .collect()
    }

    fn after(args: &[String], option: &str) -> String {
        let at = args.iter().position(|arg| arg == option).unwrap();
        args[at + 1].clone()
    }

    #[test]
    fn an_mp4_export_trims_the_input_and_encodes_h264_with_aac() {
        assert_eq!(
            of(Format::Mp4),
            expected(&[
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
                "31",
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
            ])
        );
    }

    #[test]
    fn a_webm_export_encodes_av1_with_opus() {
        assert_eq!(
            of(Format::Webm),
            expected(&[
                "-map",
                "0:a:0?",
                "-sn",
                "-dn",
                "-vf",
                EVEN,
                "-c:v",
                "av1_nvenc",
                "-preset",
                "p5",
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                "38",
                "-b:v",
                "0",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "libopus",
                "-b:a",
                "160k",
                "-f",
                "webm",
                "D:/out/x.webm.part",
            ])
        );
    }

    #[test]
    fn a_gif_export_makes_its_palette_in_the_same_call_with_no_sound() {
        let filter = format!("{EVEN},fps=15,split[a][b];[a]palettegen[p];[b][p]paletteuse");
        assert_eq!(
            of(Format::Gif),
            expected(&[
                "-an",
                "-sn",
                "-dn",
                "-vf",
                &filter,
                "-c:v",
                "gif",
                "-loop",
                "0",
                "-f",
                "gif",
                "D:/out/x.gif.part",
            ])
        );
    }

    #[test]
    fn a_webp_export_is_animated_with_no_sound() {
        let filter = format!("{EVEN},fps=20");
        assert_eq!(
            of(Format::Webp),
            expected(&[
                "-an",
                "-sn",
                "-dn",
                "-vf",
                &filter,
                "-c:v",
                "libwebp_anim",
                "-lossless",
                "0",
                "-quality",
                "55",
                "-loop",
                "0",
                "-f",
                "webp",
                "D:/out/x.webp.part",
            ])
        );
    }

    #[test]
    fn each_format_reads_its_own_controls() {
        let controls = Controls {
            mp4_quality: Quality::Lowest,
            webm_quality: Quality::Best,
            gif_fps: 10,
            webp_fps: 30,
            webp_quality: Quality::Medium,
            ..Controls::default()
        };
        let with = |format| strings(Controls { format, ..controls });
        assert_eq!(after(&with(Format::Mp4), "-cq"), "41");
        assert_eq!(after(&with(Format::Webm), "-cq"), "18");
        assert!(after(&with(Format::Gif), "-vf").contains(",fps=10,"));
        let webp = with(Format::Webp);
        assert!(after(&webp, "-vf").ends_with(",fps=30"));
        assert_eq!(after(&webp, "-quality"), "70");
    }

    #[test]
    fn the_crop_and_the_scale_follow_the_trim_in_that_order() {
        let mut edit = edit();
        edit.crop = Crop {
            x: 101,
            y: 50,
            width: 641,
            height: 481,
        };
        let vf = |edit: &Edit, format| {
            after(
                &strings_of(
                    edit,
                    Controls {
                        format,
                        ..Controls::default()
                    },
                ),
                "-vf",
            )
        };
        assert_eq!(vf(&edit, Format::Mp4), "crop=640:480:101:50");
        edit.max_width = crate::studio::MaxWidth::Pixels(360);
        assert_eq!(
            vf(&edit, Format::Mp4),
            "crop=640:480:101:50,scale=360:270:flags=lanczos"
        );
        assert_eq!(
            vf(&edit, Format::Gif),
            "crop=640:480:101:50,scale=360:270:flags=lanczos,fps=15,split[a][b];[a]palettegen[p];[b][p]paletteuse"
        );
    }

    #[test]
    fn an_uncropped_frame_scales_from_its_even_size() {
        let mut edit = edit();
        edit.max_width = crate::studio::MaxWidth::Pixels(720);
        let args = strings_of(&edit, Controls::default());
        assert_eq!(
            after(&args, "-vf"),
            format!("{EVEN},scale=720:404:flags=lanczos")
        );
        edit.size = (0, 0);
        assert_eq!(after(&strings_of(&edit, Controls::default()), "-vf"), EVEN);
    }

    #[test]
    fn the_partial_file_is_not_media() {
        let partial = partial_path(Path::new("D:/out/cat 0m01-0m02.mp4"));
        assert_eq!(partial, Path::new("D:/out/cat 0m01-0m02.mp4.part"));
        assert_eq!(crate::media::Kind::from_path(&partial), None);
    }
}
