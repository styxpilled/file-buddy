//! Playback regression tests. The fork has a history of the video freezing a
//! second or two after it starts while the audio keeps running, so these assert
//! that the picture keeps moving without a person at the mouse.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_video::Video;

/// Long enough to pass the prebuffer, the first underrun and at least one loop.
const RUN: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(100);

/// Fixture clips are not in the repository. Tests skip when they are absent.
/// Set `FIXTURE` to a path to run against one particular clip.
fn fixture() -> Option<PathBuf> {
    // Relative to the workspace root, so it reads the same as a shell path even
    // though the test's working directory is the crate.
    if let Ok(named) = std::env::var("FIXTURE") {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .to_path_buf();
        return Some(workspace.join(named));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("test_videos");
    let mut stack = vec![root];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "mp4") {
                found.push(path);
            }
        }
    }
    found.sort();
    found.into_iter().next()
}

/// A spread of clips, so a test is not confined to one frame rate or one audio
/// sample rate. The faults this suite covers only appear on some combinations:
/// the decode lead depends on the frame rate, and the resampler only holds
/// samples back when the source rate differs from the device's.
fn fixtures(count: usize) -> Vec<PathBuf> {
    let Some(first) = fixture() else {
        return Vec::new();
    };
    let root = match first.parent().and_then(|p| p.parent()) {
        Some(root) => root.to_path_buf(),
        None => return vec![first],
    };
    let mut stack = vec![root];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "mp4" || e == "mov") {
                found.push(path);
            }
        }
    }
    found.sort();
    if found.len() <= count {
        return found;
    }
    let stride = found.len() / count;
    found
        .into_iter()
        .step_by(stride.max(1))
        .take(count)
        .collect()
}

fn open(path: &PathBuf) -> Video {
    // Set RUST_LOG to see the decoder's own account of a failure.
    let _ = env_logger::try_init();
    let uri = url::Url::from_file_path(path).expect("absolute fixture path");
    let video = Video::new(&uri).expect("failed to open fixture");
    video.set_muted(true);
    video
}

/// FNV-1a over the whole frame. Sampling only the first bytes would miss motion
/// in clips whose top rows are a constant letterbox.
fn frame_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// A sample of what the decoder produced, taken every `POLL`.
struct Samples {
    /// Distinct frame payloads seen.
    distinct_frames: usize,
    /// Highest reported position.
    max_position: Duration,
    /// True if the position ever went backwards, which is what looping looks like.
    wrapped: bool,
}

fn sample(video: &Video, run: Duration) -> Samples {
    let mut last_key: Option<u64> = None;
    let mut distinct_frames = 0;
    let mut max_position = Duration::ZERO;
    let mut previous = Duration::ZERO;
    let mut wrapped = false;

    let deadline = Instant::now() + run;
    while Instant::now() < deadline {
        std::thread::sleep(POLL);

        if let Some((data, _, _)) = video.current_frame_data() {
            let key = frame_hash(&data);
            if last_key != Some(key) {
                distinct_frames += 1;
                last_key = Some(key);
            }
        }

        let position = video.position();
        if position + Duration::from_millis(200) < previous {
            wrapped = true;
        }
        previous = position;
        max_position = max_position.max(position);
    }

    Samples {
        distinct_frames,
        max_position,
        wrapped,
    }
}

#[test]
fn picture_keeps_moving_for_five_seconds() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    // Not looping: the loop reset zeroes the clock, which would hide a stall.
    video.set_paused(false);

    let samples = sample(&video, RUN);

    // Polling at 10 Hz over 5 seconds can see at most 50 frames. A freeze after
    // the prebuffer drains shows up as a handful, so this is a wide margin.
    assert!(
        samples.distinct_frames >= 25,
        "picture froze: {} distinct frames in {:?} ({})",
        samples.distinct_frames,
        RUN,
        path.display()
    );
    // The clock must keep pace with the wall clock, not race ahead of it or
    // freeze behind it.
    let expected = RUN.min(video.duration());
    assert!(
        samples.max_position >= expected.saturating_sub(Duration::from_millis(500)),
        "clock stalled at {:?}, expected about {:?} ({})",
        samples.max_position,
        expected,
        path.display()
    );
}

#[test]
fn a_paused_video_does_not_advance() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_paused(true);

    let samples = sample(&video, Duration::from_secs(1));

    assert!(
        samples.distinct_frames <= 1,
        "paused video advanced {} frames ({})",
        samples.distinct_frames,
        path.display()
    );
}

#[test]
fn looping_wraps_around() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_looping(true);
    video.set_paused(false);

    // Start near the end, so the wrap happens well inside the run.
    let duration = video.duration();
    let start = duration.saturating_sub(Duration::from_secs(1));
    video.seek(start, false).expect("seek failed");

    let samples = sample(&video, RUN);

    assert!(
        samples.wrapped,
        "never looped: reached {:?} of {:?} ({})",
        samples.max_position,
        duration,
        path.display()
    );
}

#[test]
fn seek_lands_near_the_target() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    let target = video.duration() / 2;
    video.seek(target, false).expect("seek failed");
    video.set_paused(false);

    // Long enough to refill the buffers and start presenting again.
    std::thread::sleep(Duration::from_millis(500));
    let position = video.position();

    let drift = position.max(target) - position.min(target);
    assert!(
        drift < Duration::from_millis(900),
        "seek to {:?} landed at {:?} ({})",
        target,
        position,
        path.display()
    );
}

/// A soak run. Not part of the default suite because it takes a minute.
/// `cargo test -p gpui-video --test playback -- --ignored --nocapture`
#[test]
#[ignore = "takes a minute"]
fn does_not_freeze_over_a_minute() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_frame_buffer_capacity(30);
    video.set_looping(true);
    video.set_paused(false);

    let mut last_key: Option<u64> = None;
    let mut still_since: Option<Instant> = None;
    let mut worst = Duration::ZERO;
    let deadline = Instant::now() + Duration::from_secs(60);

    while Instant::now() < deadline {
        std::thread::sleep(POLL);
        let Some((data, _, _)) = video.current_frame_data() else {
            continue;
        };
        let key = frame_hash(&data);
        if last_key == Some(key) {
            let since = *still_since.get_or_insert_with(Instant::now);
            worst = worst.max(since.elapsed());
            if since.elapsed() > Duration::from_secs(2) {
                panic!(
                    "froze for {:?} at position {:?}, {} frames buffered, audio level {:.2}",
                    since.elapsed(),
                    video.position(),
                    video.buffered_len(),
                    video.audio_buffer_level(),
                );
            }
        } else {
            last_key = Some(key);
            still_since = None;
        }
    }

    eprintln!("longest still period: {worst:?}");
}

/// What a hover tile does: play, stop and rewind, then play again. The clip has
/// to restart, not carry on from where it stopped.
#[test]
fn stop_and_rewind_restarts_playback() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_looping(true);
    video.set_paused(false);
    std::thread::sleep(Duration::from_millis(1200));
    let played_to = video.position();
    assert!(
        played_to > Duration::from_millis(500),
        "never started: {played_to:?}"
    );

    video.set_paused(true);
    video.seek(Duration::ZERO, false).expect("seek failed");
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        video.position() < Duration::from_millis(200),
        "rewind left the clock at {:?}",
        video.position()
    );

    video.set_paused(false);
    std::thread::sleep(Duration::from_millis(400));
    let restarted = video.position();
    assert!(
        restarted < played_to,
        "did not restart: resumed at {restarted:?} after stopping at {played_to:?}"
    );
}

/// The clock must move forward only, and at about wall speed. A clock that runs
/// ahead of the audio and snaps back on every device callback still averages out
/// over a second, but it makes the presenter skip and hold frames.
#[test]
fn the_clock_is_monotonic_and_steady() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_frame_buffer_capacity(30);
    video.set_looping(true);
    video.set_paused(false);

    // Let the prebuffer finish, so the measurement covers steady playback.
    std::thread::sleep(Duration::from_millis(600));

    let step = Duration::from_millis(50);
    let mut previous = video.position();
    let started = Instant::now();
    let mut worst_step = Duration::ZERO;

    while started.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(step);
        let position = video.position();
        // A loop wrap is the one legal way back.
        if position < previous {
            assert!(
                previous > video.duration() / 2,
                "clock went backwards from {previous:?} to {position:?} ({})",
                path.display()
            );
            previous = position;
            continue;
        }
        worst_step = worst_step.max(position - previous);
        previous = position;
    }

    assert!(
        worst_step < step * 4,
        "clock jumped {worst_step:?} in a {step:?} window ({})",
        path.display()
    );
}

/// Choppiness is a frame that stays on screen for several frame intervals.
#[test]
fn frames_arrive_at_about_the_frame_rate() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_frame_buffer_capacity(30);
    video.set_looping(true);
    video.set_paused(false);
    std::thread::sleep(Duration::from_millis(600));

    let step = Duration::from_millis(50);
    let mut last_key: Option<u64> = None;
    let mut polls = 0;
    let mut changes = 0;
    let started = Instant::now();

    while started.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(step);
        polls += 1;
        if let Some((data, _, _)) = video.current_frame_data() {
            let key = frame_hash(&data);
            if last_key != Some(key) {
                changes += 1;
                last_key = Some(key);
            }
        }
    }

    // The fixture runs at 25 fps and polling is at 20 Hz, so all but a rounding
    // handful of polls should see a new frame.
    assert!(
        changes * 10 >= polls * 8,
        "choppy: {changes} new frames in {polls} polls ({})",
        path.display()
    );
}

/// Prints one line a second. Run it when playback misbehaves over time:
/// `cargo test -p gpui-video --test playback diagnose -- --ignored --nocapture`
#[test]
#[ignore = "diagnostic, prints for thirty seconds"]
fn diagnose_playback_over_time() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_frame_buffer_capacity(30);
    video.set_looping(true);
    video.set_paused(false);

    eprintln!("{}, {:?}", path.display(), video.duration());
    eprintln!(" t   fps  position  vbuf  audio");

    let mut last_seq = video.frame_seq();
    let mut last_position = video.position();
    for second in 1..=30 {
        std::thread::sleep(Duration::from_secs(1));
        let seq = video.frame_seq();
        let position = video.position();
        eprintln!(
            "{second:3}  {:4}  {:7.2}s  {:4}  {:5.2}",
            seq - last_seq,
            position.as_secs_f32(),
            video.buffered_len(),
            video.audio_buffer_level(),
        );
        last_seq = seq;
        last_position = position;
    }
    let _ = last_position;
}

/// The fault this catches took ten seconds to show, and only on a clip whose
/// frame rate fills the frame buffer faster than the audio ring fills, so it
/// measures every second and runs over a spread of clips.
#[test]
#[ignore = "takes about a minute"]
fn the_presented_frame_rate_holds() {
    let clips = fixtures(4);
    if clips.is_empty() {
        eprintln!("skipped: no fixtures under test_videos/");
        return;
    }

    for path in clips {
        let video = open(&path);
        // The element caps the buffer, and the cap is what made the rate decay.
        video.set_frame_buffer_capacity(30);
        video.set_looping(true);
        video.set_paused(false);
        std::thread::sleep(Duration::from_millis(800));

        let expected = video.framerate();
        let started = video.frame_seq();
        let mut last_seq = started;
        let mut worst = f64::MAX;

        const SECONDS: u64 = 15;
        for _ in 0..SECONDS {
            std::thread::sleep(Duration::from_secs(1));
            let seq = video.frame_seq();
            worst = worst.min((seq - last_seq) as f64);
            last_seq = seq;
        }

        let average = (last_seq - started) as f64 / SECONDS as f64;
        assert!(
            average >= expected * 0.85,
            "{}: averaged {average:.1} fps, clip runs at {expected:.1} fps",
            path.display()
        );
        // The floor is loose because restarting a loop drains and refills both
        // buffers, which costs a fraction of a second. See the known issues in
        // README.md.
        assert!(
            worst >= expected * 0.5,
            "{}: slowest second presented {worst} frames, clip runs at {expected:.1} fps",
            path.display()
        );
    }
}

/// Hovering a tile, leaving, and coming back later. The clock must not count the
/// idle time: a pause is not a stall, but the audio device consumes nothing
/// while paused, so the two look alike from inside the decoder.
#[test]
fn resuming_after_a_long_pause_does_not_race() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_frame_buffer_capacity(30);
    video.set_looping(true);

    video.set_paused(false);
    std::thread::sleep(Duration::from_millis(1200));
    let paused_at = video.position();

    video.set_paused(true);
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        video.position() < paused_at + Duration::from_millis(200),
        "clock ran on while paused: {:?} to {:?}",
        paused_at,
        video.position()
    );

    let before = video.frame_seq();
    video.set_paused(false);
    std::thread::sleep(Duration::from_millis(500));

    let advanced = video.position().saturating_sub(paused_at);
    assert!(
        advanced < Duration::from_millis(1500),
        "clock jumped {advanced:?} on resume, as if the idle time had played"
    );

    let presented = (video.frame_seq() - before) as f64;
    assert!(
        presented < video.framerate(),
        "raced through {presented} frames in half a second at {:.1} fps",
        video.framerate()
    );
}

/// The exact sequence a hover tile performs: stop and rewind, idle, hover again.
#[test]
fn rehovering_after_an_idle_gap_plays_at_normal_speed() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_frame_buffer_capacity(30);
    video.set_looping(true);

    video.set_paused(false);
    std::thread::sleep(Duration::from_millis(1200));

    // Leaving the tile.
    video.set_paused(true);
    video.seek(Duration::ZERO, false).expect("seek failed");
    std::thread::sleep(Duration::from_secs(3));

    // Hovering it again.
    let before = video.frame_seq();
    video.set_paused(false);
    std::thread::sleep(Duration::from_millis(500));

    let presented = (video.frame_seq() - before) as f64;
    assert!(
        presented < video.framerate(),
        "raced through {presented} frames in half a second at {:.1} fps ({})",
        video.framerate(),
        path.display()
    );
    assert!(
        video.position() < Duration::from_millis(900),
        "restarted at {:?} instead of the beginning",
        video.position()
    );
}

/// The race is narrow: it needs the decoder to look at the clock after a resume
/// before the audio device has consumed anything, so it takes repetition.
#[test]
#[ignore = "stress, takes about a minute"]
fn repeated_hover_cycles_never_race() {
    let Some(path) = fixture() else {
        eprintln!("skipped: no fixture under test_videos/");
        return;
    };
    let video = open(&path);
    video.set_frame_buffer_capacity(30);
    video.set_looping(true);
    let rate = video.framerate();

    for attempt in 0..25 {
        video.set_paused(false);
        std::thread::sleep(Duration::from_millis(700));
        video.set_paused(true);
        video.seek(Duration::ZERO, false).expect("seek failed");
        // Vary the idle, so the resume lands at different points of the audio
        // device's own callback period.
        std::thread::sleep(Duration::from_millis(900 + attempt * 7));

        let before = video.frame_seq();
        let resumed_at = Instant::now();
        video.set_paused(false);

        // Sampling throughout catches the jump itself. Counting presented frames
        // alone does not: a clock that leaps forward makes the decoder *skip*
        // the frames it has passed, and a skip is not a present.
        while resumed_at.elapsed() < Duration::from_millis(400) {
            std::thread::sleep(Duration::from_millis(20));
            let position = video.position();
            let allowed = resumed_at.elapsed() + Duration::from_millis(250);
            assert!(
                position <= allowed,
                "attempt {attempt}: clock reached {position:?} after {:?} of playing",
                resumed_at.elapsed()
            );
        }

        let presented = (video.frame_seq() - before) as f64;
        assert!(
            presented < rate,
            "attempt {attempt}: raced through {presented} frames in 400 ms at {rate:.1} fps"
        );
    }
}

/// Wait until the seek has put a frame on screen. `None` when it takes longer
/// than `limit`.
fn wait_for_seek(video: &Video, limit: Duration) -> Option<Duration> {
    let started = Instant::now();
    while video.seeking() {
        if started.elapsed() > limit {
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Some(started.elapsed())
}

fn wait_for_first_frame(video: &Video) {
    let started = Instant::now();
    while video.frame_seq() == 0 && started.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A paused clip, as the side pane holds it, shows the frame at the target.
#[test]
fn an_accurate_seek_shows_the_frame_at_the_target() {
    for path in fixtures(6) {
        let video = open(&path);
        video.set_paused(true);
        wait_for_first_frame(&video);

        let target = video.duration().mul_f64(0.6);
        video.seek(target, true).expect("seek failed");
        wait_for_seek(&video, Duration::from_secs(3))
            .unwrap_or_else(|| panic!("seek never showed a frame ({})", path.display()));

        // Frame times are not always evenly spaced, so the bound is loose. A
        // keyframe seek misses by seconds.
        let shown = video.frame_time();
        assert!(
            shown <= target && target - shown < Duration::from_millis(100),
            "seek to {target:?} showed the frame at {shown:?} ({})",
            path.display()
        );
    }
}

/// How long a seek takes to show its frame, over every clip in `test_videos`:
/// to a keyframe while paused, then accurate while paused and while playing.
/// `cargo test --test playback seek_times -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, not a check"]
fn seek_times() {
    for path in fixtures(usize::MAX) {
        let video = open(&path);
        video.set_paused(true);
        wait_for_first_frame(&video);
        let (width, height) = video.size();
        let mut row = String::new();
        for (accurate, paused) in [(false, true), (true, true), (true, false)] {
            video.set_paused(paused);
            let mut worst = Duration::ZERO;
            let mut total = Duration::ZERO;
            for share in [0.25, 0.5, 0.9] {
                video
                    .seek(video.duration().mul_f64(share), accurate)
                    .unwrap();
                let took = wait_for_seek(&video, Duration::from_secs(5)).unwrap_or(Duration::MAX);
                worst = worst.max(took);
                total += took;
            }
            row += &format!(
                "  {:>5.0} / {:>5.0} ms",
                total.as_secs_f64() * 1000.0 / 3.0,
                worst.as_secs_f64() * 1000.0
            );
        }
        println!(
            "{:>4}x{:<4} {:>5.1}s {row}  {}",
            width,
            height,
            video.duration().as_secs_f64(),
            path.file_name().unwrap().to_string_lossy()
        );
    }
}
