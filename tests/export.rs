//! One range of a generated clip, exported in each format and probed. NVENC
//! needs the GPU.

use std::path::Path;
use std::process::Command;

use file_buddy::ffmpeg::probe::{Probe, probe};
use file_buddy::ffmpeg::{self, Tool, export, webp};
use file_buddy::media::Kind;
use file_buddy::studio::frames;
use file_buddy::studio::sequence::{self, Place};
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

/// Four frames of one color each, red, green, blue and yellow.
fn gif_fixture(path: &Path, delays: [u32; 4]) {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, Rgba, RgbaImage};
    let colors = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = GifEncoder::new(file);
    encoder.set_repeat(Repeat::Infinite).unwrap();
    let frames = colors.iter().zip(delays).map(|([r, g, b], ms)| {
        let image = RgbaImage::from_pixel(64, 48, Rgba([*r, *g, *b, 255]));
        Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(ms, 1))
    });
    encoder.encode_frames(frames).unwrap();
}

/// Each frame as its color, and its delay.
fn colors_and_delays(frames: &[frames::Frame]) -> Vec<(char, u32)> {
    frames
        .iter()
        .map(|frame| {
            let [r, g, b, _] = frame.image.get_pixel(32, 24).0;
            let color = match (r > 128, g > 128, b > 128) {
                (true, false, false) => 'r',
                (false, true, false) => 'g',
                (false, false, true) => 'b',
                (true, true, false) => 'y',
                _ => '?',
            };
            (color, frame.delay_ms)
        })
        .collect()
}

#[test]
fn frames_cut_from_a_gif_keep_their_delays_in_each_format() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("cat.gif");
    gif_fixture(&source, [40, 250, 70, 500]);
    let decoded = frames::decode(&source).unwrap();
    assert_eq!(
        colors_and_delays(&decoded),
        [('r', 40), ('g', 250), ('b', 70), ('y', 500)]
    );

    let mut edit = Edit::with_frames(source, decoded);
    let one = |index| std::collections::BTreeSet::from([index]);
    edit.delete_frames(&one(1));
    edit.duplicate_frames(&one(0));
    edit.move_frames(&one(1), 3);
    edit.set_delay(&one(1), 120);
    let expected = [('r', 40), ('b', 120), ('r', 40), ('y', 500)];
    assert_eq!(colors_and_delays(&edit.frames), expected);

    // A screenshot takes the frame of the strip at its moment: blue, from 40 ms.
    let shot = export::screenshot(&edit, 100).unwrap();
    let shot = image::load_from_memory(&shot).unwrap().to_rgba8();
    assert_eq!(shot.dimensions(), (64, 48));
    assert_eq!(shot.get_pixel(32, 24).0, [0, 0, 255, 255]);

    for format in [Format::Gif, Format::Webp] {
        let destination = dir.path().join(format!("out.{}", format.extension()));
        let controls = Controls {
            format,
            ..Controls::default()
        };
        export::run(&edit, &controls, &destination, |_| {}).expect("export");
        let exported = frames::decode(&destination).unwrap();
        assert_eq!(colors_and_delays(&exported), expected, "{format:?}");
    }

    // The range and a skip leave frames out, and the rest keep their delays.
    let mut cut = edit.clone();
    cut.set_start(40);
    cut.toggle_skip(&one(2));
    let destination = dir.path().join("cut.gif");
    let controls = Controls {
        format: Format::Gif,
        ..Controls::default()
    };
    export::run(&cut, &controls, &destination, |_| {}).expect("export");
    let exported = frames::decode(&destination).unwrap();
    assert_eq!(colors_and_delays(&exported), [('b', 120), ('y', 500)]);

    let destination = dir.path().join("out.mp4");
    let controls = Controls {
        format: Format::Mp4,
        ..Controls::default()
    };
    // NVENC takes no frame this small.
    edit.max_width = MaxWidth::Source;
    let mut large = edit.clone();
    for frame in &mut large.frames {
        let color = *frame.image.get_pixel(0, 0);
        frame.image = std::sync::Arc::new(image::RgbaImage::from_pixel(320, 240, color));
    }
    large.size = (320, 240);
    large.crop = Crop::full(large.size);
    export::run(&large, &controls, &destination, |_| {}).expect("export");
    let probe = probe(&destination, Kind::Video).unwrap();
    let duration = probe.duration.expect("a duration");
    assert!((duration - 0.7).abs() < 0.02, "{duration} s");
}

#[test]
fn a_sequence_has_transparent_bars_in_gif_and_webp_and_black_in_mp4() {
    use image::{Rgba, RgbaImage};
    use std::collections::BTreeSet;

    let dir = tempfile::tempdir().unwrap();
    let wide = dir.path().join("a wide.png");
    let tall = dir.path().join("b tall.png");
    RgbaImage::from_pixel(320, 120, Rgba([255, 0, 0, 255]))
        .save(&wide)
        .unwrap();
    RgbaImage::from_pixel(120, 320, Rgba([0, 0, 255, 255]))
        .save(&tall)
        .unwrap();
    let mut stills = Vec::new();
    sequence::decode_each(&[wide.clone(), tall], |frame| {
        stills.push(frame);
        true
    })
    .unwrap();
    let mut edit = Edit::sequence(wide, stills);
    assert_eq!(edit.size, (320, 120));
    assert_eq!(edit.duration_ms, 200);
    edit.set_canvas((320, 240));
    // The tall frame moves a quarter of the canvas right.
    edit.move_placed(&[(1, Place::default())], 80., 0., 0.);
    edit.scale_placed(&BTreeSet::from([0]), 1.);

    let alpha = |image: &RgbaImage, x, y| image.get_pixel(x, y)[3];
    for format in [Format::Gif, Format::Webp] {
        let destination = dir.path().join(format!("out.{}", format.extension()));
        let controls = Controls {
            format,
            ..Controls::default()
        };
        export::run(&edit, &controls, &destination, |_| {}).expect("export");
        let exported = frames::decode(&destination).unwrap();
        assert_eq!(exported.len(), 2, "{format:?}");
        let (first, second) = (&exported[0].image, &exported[1].image);
        assert_eq!(first.dimensions(), (320, 240), "{format:?}");
        assert_eq!(alpha(first, 160, 20), 0, "{format:?}");
        assert!(first.get_pixel(160, 120)[0] > 200, "{format:?}");
        // The tall image fits at 90 x 240, from 115, and moves 80 right.
        assert_eq!(alpha(second, 190, 120), 0, "{format:?}");
        assert!(second.get_pixel(240, 120)[2] > 200, "{format:?}");
        assert_eq!(alpha(second, 290, 120), 0, "{format:?}");
    }

    // NVENC takes no frame this small.
    edit.set_canvas((320, 240));
    let large = Edit {
        max_width: MaxWidth::Source,
        ..edit.clone()
    };
    let destination = dir.path().join("out.mp4");
    let controls = Controls {
        format: Format::Mp4,
        ..Controls::default()
    };
    export::run(&large, &controls, &destination, |_| {}).expect("export");
    let first = sequence::decode(&destination).unwrap();
    assert_eq!(first.dimensions(), (320, 240));
    let bar = first.get_pixel(160, 20).0;
    assert!(bar[..3].iter().all(|&c| c < 20), "{bar:?}");
    assert!(first.get_pixel(160, 120)[0] > 200);
}
