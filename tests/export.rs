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
