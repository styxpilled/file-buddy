//! One range of a generated clip, exported in each format and probed. NVENC
//! needs the GPU.

use std::path::Path;
use std::process::Command;

use file_buddy::ffmpeg::probe::{Probe, probe};
use file_buddy::ffmpeg::{self, Tool, export, webp};
use file_buddy::media::Kind;
use file_buddy::studio::{Controls, Crop, Edit, Format, MaxWidth, Range};

fn fixture(path: &Path) {
    let output = Command::new(ffmpeg::path_of(Tool::Ffmpeg))
        .args(["-nostdin", "-v", "error", "-y"])
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=3",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
        .arg(path)
        .output()
        .expect("run ffmpeg");
    assert!(
        output.status.success(),
        "ffmpeg failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn export_as(dir: &Path, format: Format) -> std::path::PathBuf {
    let source = dir.join("source.mp4");
    if !source.exists() {
        fixture(&source);
    }
    let mut edit = Edit::new(source.clone(), source, 3_000, (320, 240));
    edit.range = Range {
        start_ms: 500,
        end_ms: 2_500,
    };
    let controls = Controls {
        format,
        ..Controls::default()
    };
    let destination = dir.join(format!("out.{}", format.extension()));
    export::run(&edit, &controls, &destination, |_| {}).expect("export");
    destination
}

fn assert_two_seconds(probe: &Probe) {
    let duration = probe.duration.expect("a duration");
    assert!((duration - 2.).abs() < 0.15, "{duration} s");
    assert_eq!((probe.width, probe.height), (Some(320), Some(240)));
}

#[test]
fn each_format_exports_a_range_that_probes() {
    let dir = tempfile::tempdir().unwrap();

    let mp4 = probe(&export_as(dir.path(), Format::Mp4), Kind::Video).unwrap();
    assert_eq!(mp4.video_codec.as_deref(), Some("h264"));
    assert_eq!(mp4.audio_codec.as_deref(), Some("aac"));
    assert_two_seconds(&mp4);

    let webm = probe(&export_as(dir.path(), Format::Webm), Kind::Video).unwrap();
    assert_eq!(webm.video_codec.as_deref(), Some("av1"));
    assert_eq!(webm.audio_codec.as_deref(), Some("opus"));
    assert_two_seconds(&webm);

    let gif = probe(&export_as(dir.path(), Format::Gif), Kind::Animated).unwrap();
    assert_eq!(gif.video_codec.as_deref(), Some("gif"));
    assert_eq!(gif.audio_codec, None);
    assert_two_seconds(&gif);

    // FFmpeg cannot read an animated WebP, so the index reads it through a proxy.
    let path = export_as(dir.path(), Format::Webp);
    assert!(webp::is_animated(&path).unwrap());
    let cache = dir.path().join("cache");
    let proxy = webp::write_proxy(&path, "abcd", &cache).unwrap();
    assert_eq!(proxy.audio_codec, None);
    assert_two_seconds(&proxy);
}

#[test]
fn a_speed_change_keeps_the_sound_at_the_new_length() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    fixture(&source);
    for (speed, length) in [(0.5, 6.), (4., 0.75)] {
        let mut edit = Edit::new(source.clone(), source.clone(), 3_000, (320, 240));
        edit.speed = speed;
        let destination = dir.path().join(format!("out {speed}.mp4"));
        export::run(&edit, &Controls::default(), &destination, |_| {}).expect("export");
        let probe = probe(&destination, Kind::Video).unwrap();
        assert_eq!(probe.audio_codec.as_deref(), Some("aac"));
        let duration = probe.duration.expect("a duration");
        assert!((duration - length).abs() < 0.15, "{speed}x: {duration} s");
    }
}

#[test]
fn a_text_with_filter_characters_in_its_words_exports() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    fixture(&source);
    let mut edit = Edit::new(source.clone(), source, 3_000, (320, 240));
    let at = edit.add_text();
    edit.texts[at].words = "it's 5:00, [ok]; a\\b %{n}\nline two".into();
    edit.texts[at].boxed = true;
    edit.texts[at].start_ms = 1_000;
    for format in [Format::Mp4, Format::Gif] {
        let destination = dir.path().join(format!("out.{}", format.extension()));
        let controls = Controls {
            format,
            ..Controls::default()
        };
        export::run(&edit, &controls, &destination, |_| {}).expect("export");
        assert!(probe(&destination, Kind::Video).is_ok());
    }
}

#[test]
fn a_still_with_a_crop_and_a_text_exports_as_png_and_jpg() {
    let dir = tempfile::tempdir().unwrap();
    // A `%` in the name must not read as a pattern.
    let source = dir.path().join("still 100%.png");
    let output = Command::new(ffmpeg::path_of(Tool::Ffmpeg))
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .args(["testsrc2=size=640x480,format=rgba", "-frames:v", "1"])
        .arg(&source)
        .output()
        .expect("run ffmpeg");
    assert!(output.status.success());
    let mut edit = Edit::still(source, (640, 480));
    edit.crop = Crop {
        x: 40,
        y: 20,
        width: 401,
        height: 300,
    };
    edit.max_width = MaxWidth::Pixels(200);
    let at = edit.add_text();
    edit.texts[at].words = "it's 5:00\nline two".into();
    edit.texts[at].boxed = true;
    for (format, codec) in [(Format::Png, "png"), (Format::Jpg, "mjpeg")] {
        let destination = dir.path().join(format!("out 100%.{}", format.extension()));
        let controls = Controls {
            format,
            ..Controls::default()
        };
        export::run(&edit, &controls, &destination, |_| {}).expect("export");
        let probe = probe(&destination, Kind::Still).unwrap();
        assert_eq!(probe.kind, Kind::Still);
        assert_eq!(probe.video_codec.as_deref(), Some(codec));
        assert_eq!((probe.width, probe.height), (Some(200), Some(150)));
    }
}

#[test]
fn a_screenshot_is_a_png_of_the_crop_at_its_own_size() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    fixture(&source);
    let mut edit = Edit::new(source.clone(), source.clone(), 3_000, (320, 240));
    let raw = export::screenshot(&edit, 1_240).unwrap();
    let raw = image::load_from_memory(&raw).unwrap();
    assert_eq!((raw.width(), raw.height()), (320, 240));
    // A seek after the input decodes every frame up to it, so it is exact.
    let output = Command::new(ffmpeg::path_of(Tool::Ffmpeg))
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(&source)
        .args(["-ss", "1.240", "-frames:v", "1", "-c:v", "png"])
        .args(["-f", "image2pipe", "pipe:1"])
        .output()
        .expect("run ffmpeg");
    let exact = image::load_from_memory(&output.stdout).unwrap();
    assert!(raw.to_rgb8() == exact.to_rgb8(), "not the frame at 1.24 s");

    edit.crop = Crop {
        x: 40,
        y: 20,
        width: 201,
        height: 151,
    };
    edit.max_width = MaxWidth::Pixels(100);
    let at = edit.add_text();
    edit.texts[at].words = "it's 5:00".into();
    let cropped = export::screenshot(&edit, 2_960).unwrap();
    let cropped = image::load_from_memory(&cropped).unwrap();
    assert_eq!((cropped.width(), cropped.height()), (201, 151));
}

#[test]
fn a_crop_and_a_scale_set_the_frame_size() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    fixture(&source);
    let mut edit = Edit::new(source.clone(), source, 3_000, (320, 240));
    edit.crop = Crop {
        x: 40,
        y: 20,
        width: 201,
        height: 150,
    };
    edit.max_width = MaxWidth::Pixels(160);
    let destination = dir.path().join("out.mp4");
    export::run(&edit, &Controls::default(), &destination, |_| {}).expect("export");
    let probe = probe(&destination, Kind::Video).unwrap();
    assert_eq!((probe.width, probe.height), (Some(160), Some(120)));
}
