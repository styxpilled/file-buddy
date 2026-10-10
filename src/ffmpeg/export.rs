//! The Studio export: the FFmpeg arguments from an edit, and the run that
//! writes the file. The filters go in the order trim, crop, scale, text, speed.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::ffmpeg::{self, Tool};
use crate::studio::{self, Controls, Crop, Edit, Format, MaxWidth, Quality, Text};

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

/// The `mjpeg` scale, from 2 to 31. Lower is better and larger.
fn jpg_q(quality: Quality) -> u8 {
    match quality {
        Quality::Best => 2,
        Quality::High => 3,
        Quality::Medium => 5,
        Quality::Low => 8,
        Quality::Lower => 12,
        Quality::Lowest => 18,
    }
}

/// Everything after the shared options of `ffmpeg::command`. The trim is an
/// input option, so FFmpeg does not decode what comes before the in point. A
/// still has no trim.
pub fn args(edit: &Edit, controls: &Controls, output: &Path) -> Vec<OsString> {
    let range = edit.range;
    let mut args = Args::default();
    if !edit.still {
        args.push(&[
            "-ss",
            &seconds(range.start_ms),
            "-t",
            &seconds(range.len_ms()),
        ]);
    }
    args.push(&["-i"]);
    args.0.push(edit.input.clone().into_os_string());
    args.push(&["-map", "0:v:0"]);
    match controls.format {
        Format::Mp4 | Format::Webm => args.push(&["-map", "0:a:0?"]),
        Format::Gif | Format::Webp | Format::Png | Format::Jpg => args.push(&["-an"]),
    }
    args.push(&["-sn", "-dn", "-vf", &video_filter(edit, controls)]);
    let sound = matches!(controls.format, Format::Mp4 | Format::Webm);
    if sound && edit.speed != 1. {
        args.push(&["-af", &atempo(edit.speed)]);
    }
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
        // `-update` takes the name as it is, so a `%` in it is no pattern.
        Format::Png => args.push(&["-frames:v", "1", "-c:v", "png", "-update", "1"]),
        Format::Jpg => {
            let q = jpg_q(controls.jpg_quality).to_string();
            args.push(&["-frames:v", "1", "-c:v", "mjpeg", "-q:v", &q]);
            args.push(&["-pix_fmt", "yuvj444p", "-update", "1"]);
        }
    }
    if controls.format.is_still() {
        args.push(&["-f", "image2"]);
    }
    args.0.push(output.as_os_str().to_owned());
    args.0
}

/// A GIF makes its palette from its own frames, in the same FFmpeg call. The
/// frame rate applies after the speed change.
fn video_filter(edit: &Edit, controls: &Controls) -> String {
    let mut filter = frame_filter(edit);
    for text in &edit.texts {
        if let Some(drawtext) = text_filter(edit, text) {
            filter.push(',');
            filter.push_str(&drawtext);
        }
    }
    if edit.speed != 1. {
        filter.push_str(&format!(",setpts=PTS/{}", edit.speed));
    }
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

/// One text, centered on its place and shown in its time. The times are
/// before the speed change, because the filter comes before it. `y_align=font`
/// puts the ascent at `y` on every line, as GPUI does a little lower.
fn text_filter(edit: &Edit, text: &Text) -> Option<String> {
    if edit.size.0 == 0 || edit.size.1 == 0 || text.trimmed().is_empty() {
        return None;
    }
    let enable = if edit.still {
        String::new()
    } else {
        let (start, end) = edit.text_window(text)?;
        format!(
            ":enable={}",
            escape(&format!("between(t,{start:.3},{end:.3})"))
        )
    };
    let output = edit.output_size();
    let font_px = text.font_px(output.1);
    let (x, y) = text.center_px(output);
    let y = y + Text::ascent_offset_px(font_px);
    let mut filter = format!(
        "drawtext=fontfile={}:expansion=none:text={}:fontsize={font_px}:fontcolor=white\
         :text_align=C:y_align=font:x={x:.2}-text_w/2:y={y:.2}-text_h/2{enable}",
        escape(studio::FONT_FILE),
        escape(text.trimmed()),
    );
    if text.boxed {
        filter.push_str(&format!(
            ":box=1:boxcolor=black:boxborderw={}",
            Text::padding_px(font_px)
        ));
    }
    Some(filter)
}

/// A value inside a filter option, which FFmpeg unescapes twice: once for the
/// filter graph and once for the options of the filter.
fn escape(value: &str) -> String {
    let mut option = String::new();
    for c in value.chars() {
        if matches!(c, '\\' | '\'' | ':') {
            option.push('\\');
        }
        option.push(c);
    }
    let mut graph = String::new();
    for c in option.chars() {
        if matches!(c, '\\' | '\'' | '[' | ']' | ',' | ';') {
            graph.push('\\');
        }
        graph.push(c);
    }
    graph
}

/// `atempo` keeps the pitch. Above 2x one filter skips samples, so the chain
/// takes steps of 2x.
fn atempo(speed: f64) -> String {
    let mut steps = Vec::new();
    let mut rest = speed;
    while rest > 2. {
        steps.push("atempo=2".to_string());
        rest /= 2.;
    }
    steps.push(format!("atempo={rest}"));
    steps.join(",")
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

/// `p`. One frame at `at_ms` of the source as PNG on standard output, with
/// the crop and the texts that show then. A seek before the input is exact,
/// because FFmpeg decodes and drops the frames up to it.
pub fn screenshot_args(edit: &Edit, at_ms: u64) -> Vec<OsString> {
    let mut args = Args::default();
    if !edit.still {
        args.push(&["-ss", &seconds(at_ms)]);
    }
    args.push(&["-i"]);
    args.0.push(edit.input.clone().into_os_string());
    args.push(&["-map", "0:v:0", "-an", "-sn", "-dn"]);
    let filter = screenshot_filter(edit, at_ms);
    if !filter.is_empty() {
        args.push(&["-vf", &filter]);
    }
    args.push(&[
        "-frames:v",
        "1",
        "-c:v",
        "png",
        "-f",
        "image2pipe",
        "pipe:1",
    ]);
    args.0
}

/// The crop at its own size: no scale, and no pixel lost to an even size.
fn screenshot_filter(edit: &Edit, at_ms: u64) -> String {
    let frame = Edit {
        max_width: MaxWidth::Source,
        still: true,
        ..edit.clone()
    };
    let mut filters = Vec::new();
    let crop = edit.crop;
    if edit.size.0 > 0 && edit.size.1 > 0 && crop != Crop::full(edit.size) {
        filters.push(format!(
            "crop={}:{}:{}:{}:exact=1",
            crop.width, crop.height, crop.x, crop.y
        ));
    }
    filters.extend(
        edit.texts
            .iter()
            .filter(|text| edit.shows(text, at_ms))
            .filter_map(|text| text_filter(&frame, text)),
    );
    filters.join(",")
}

/// The PNG bytes of one frame.
pub fn screenshot(edit: &Edit, at_ms: u64) -> Result<Vec<u8>> {
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command.args(screenshot_args(edit, at_ms));
    let png = ffmpeg::output(command).context("extract the frame")?;
    if png.is_empty() {
        bail!("FFmpeg gave no frame");
    }
    Ok(png)
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

    let length_us = (edit.output_ms() * 1000).max(1) as f32;
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
    fn the_speed_goes_last_in_the_video_and_the_sound() {
        let at = |speed, format| {
            let mut edit = edit();
            edit.speed = speed;
            strings_of(
                &edit,
                Controls {
                    format,
                    ..Controls::default()
                },
            )
        };
        let sound = |args: &[String]| {
            args.iter()
                .position(|arg| arg == "-af")
                .map(|at| args[at + 1].clone())
        };

        let mp4 = at(1., Format::Mp4);
        assert_eq!(after(&mp4, "-vf"), EVEN);
        assert_eq!(sound(&mp4), None);

        let mp4 = at(0.5, Format::Mp4);
        assert_eq!(after(&mp4, "-vf"), format!("{EVEN},setpts=PTS/0.5"));
        assert_eq!(sound(&mp4).as_deref(), Some("atempo=0.5"));
        let webm = at(4., Format::Webm);
        assert_eq!(after(&webm, "-vf"), format!("{EVEN},setpts=PTS/4"));
        assert_eq!(sound(&webm).as_deref(), Some("atempo=2,atempo=2"));
        assert_eq!(
            sound(&at(3., Format::Mp4)).as_deref(),
            Some("atempo=2,atempo=1.5")
        );

        // No sound to change, and the frame rate follows the speed.
        let gif = at(4., Format::Gif);
        assert_eq!(sound(&gif), None);
        assert_eq!(
            after(&gif, "-vf"),
            format!("{EVEN},setpts=PTS/4,fps=15,split[a][b];[a]palettegen[p];[b][p]paletteuse")
        );
        let webp = at(0.5, Format::Webp);
        assert_eq!(sound(&webp), None);
        assert_eq!(after(&webp, "-vf"), format!("{EVEN},setpts=PTS/0.5,fps=20"));
        assert!(webp.contains(&"-an".to_string()));
    }

    #[test]
    fn a_value_is_escaped_for_the_graph_and_for_the_filter() {
        assert_eq!(
            escape("C:/Windows/Fonts/arialbd.ttf"),
            r"C\\:/Windows/Fonts/arialbd.ttf"
        );
        assert_eq!(escape(r"a:b'c\d"), r"a\\:b\\\'c\\\\d");
        assert_eq!(escape("e,f[g];h\ni"), "e\\,f\\[g\\]\\;h\ni");
        assert_eq!(escape("between(t,0,1)"), r"between(t\,0\,1)");
    }

    #[test]
    fn a_text_is_drawn_after_the_scale_and_before_the_speed() {
        let mut edit = edit();
        edit.crop = Crop {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
        };
        edit.max_width = crate::studio::MaxWidth::Pixels(640);
        edit.speed = 2.;
        let at = edit.add_text();
        edit.texts[at].words = " it's 5:00 \n".into();
        edit.texts[at].boxed = true;
        edit.texts[at].start_ms = 4_000;
        // 8% of 360 is 28.8, so 29 px with a 7 px box.
        assert_eq!(
            after(&strings_of(&edit, Controls::default()), "-vf"),
            concat!(
                r"crop=1280:720:0:0,scale=640:360:flags=lanczos,",
                r"drawtext=fontfile=C\\:/Windows/Fonts/arialbd.ttf:expansion=none",
                r":text=it\\\'s 5\\:00:fontsize=29:fontcolor=white:text_align=C:y_align=font",
                r":x=320.00-text_w/2:y=180.48-text_h/2:enable=between(t\,0.750\,3.750)",
                r":box=1:boxcolor=black:boxborderw=7,setpts=PTS/2",
            )
        );
    }

    #[test]
    fn a_text_outside_the_range_or_with_no_words_is_not_drawn() {
        let mut edit = edit();
        let at = edit.add_text();
        edit.texts[at].end_ms = 3_000;
        let none = edit.add_text();
        edit.texts[none].words = "  \n ".into();
        assert_eq!(after(&strings_of(&edit, Controls::default()), "-vf"), EVEN);
    }

    #[test]
    fn a_still_with_a_crop_and_a_text_is_one_frame_with_no_time() {
        let mut edit = Edit::still("D:/pics/cat.png".into(), (1920, 1080));
        edit.crop = Crop {
            x: 100,
            y: 50,
            width: 1280,
            height: 720,
        };
        edit.max_width = crate::studio::MaxWidth::Pixels(640);
        let at = edit.add_text();
        edit.texts[at].words = "hi".into();
        let filter = concat!(
            r"crop=1280:720:100:50,scale=640:360:flags=lanczos,",
            r"drawtext=fontfile=C\\:/Windows/Fonts/arialbd.ttf:expansion=none",
            r":text=hi:fontsize=29:fontcolor=white:text_align=C:y_align=font",
            r":x=320.00-text_w/2:y=180.48-text_h/2",
        );
        let with = |format| {
            strings_of(
                &edit,
                Controls {
                    format,
                    ..Controls::default()
                },
            )
        };
        let start = [
            "-i",
            "D:/pics/cat.png",
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            filter,
        ];
        let expect = |rest: &[&str]| -> Vec<String> {
            start
                .iter()
                .chain(rest)
                .map(|arg| arg.to_string())
                .collect()
        };
        assert_eq!(
            with(Format::Png),
            expect(&[
                "-frames:v",
                "1",
                "-c:v",
                "png",
                "-update",
                "1",
                "-f",
                "image2",
                "D:/out/x.png.part"
            ])
        );
        assert_eq!(
            with(Format::Jpg),
            expect(&[
                "-frames:v",
                "1",
                "-c:v",
                "mjpeg",
                "-q:v",
                "3",
                "-pix_fmt",
                "yuvj444p",
                "-update",
                "1",
                "-f",
                "image2",
                "D:/out/x.jpg.part",
            ])
        );
        let lowest = strings_of(
            &edit,
            Controls {
                format: Format::Jpg,
                jpg_quality: Quality::Lowest,
                ..Controls::default()
            },
        );
        assert_eq!(after(&lowest, "-q:v"), "18");
    }

    #[test]
    fn a_screenshot_has_the_crop_at_its_own_size_and_the_texts_of_its_moment() {
        let mut edit = edit();
        edit.crop = Crop {
            x: 101,
            y: 50,
            width: 641,
            height: 481,
        };
        edit.max_width = MaxWidth::Pixels(320);
        edit.speed = 2.;
        let shown = edit.add_text();
        edit.texts[shown].words = "now".into();
        let later = edit.add_text();
        edit.texts[later].words = "later".into();
        edit.texts[later].start_ms = 6_000;
        let args: Vec<String> = screenshot_args(&edit, 4_000)
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect();
        let filter = concat!(
            r"crop=641:481:101:50:exact=1,",
            r"drawtext=fontfile=C\\:/Windows/Fonts/arialbd.ttf:expansion=none",
            r":text=now:fontsize=38:fontcolor=white:text_align=C:y_align=font",
            r":x=320.00-text_w/2:y=240.62-text_h/2",
        );
        let expected = [
            "-ss",
            "4.000",
            "-i",
            "D:/cache/ab/abcd.proxy.mp4",
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            filter,
            "-frames:v",
            "1",
            "-c:v",
            "png",
            "-f",
            "image2pipe",
            "pipe:1",
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn a_raw_screenshot_has_no_filter_and_a_still_no_seek() {
        let strings = |edit: &Edit| -> Vec<String> {
            screenshot_args(edit, 1_500)
                .into_iter()
                .map(|arg| arg.into_string().unwrap())
                .collect()
        };
        let pane = Edit::new("a.mp4".into(), "a.mp4".into(), 0, (0, 0));
        let args = strings(&pane);
        assert_eq!(&args[..4], ["-ss", "1.500", "-i", "a.mp4"]);
        assert!(!args.contains(&"-vf".to_string()));
        let still = Edit::still("b.png".into(), (640, 480));
        assert_eq!(&strings(&still)[..2], ["-i", "b.png"]);
    }

    #[test]
    fn the_partial_file_is_not_media() {
        let partial = partial_path(Path::new("D:/out/cat 0m01-0m02.mp4"));
        assert_eq!(partial, Path::new("D:/out/cat 0m01-0m02.mp4.part"));
        assert_eq!(crate::media::Kind::from_path(&partial), None);
    }
}
