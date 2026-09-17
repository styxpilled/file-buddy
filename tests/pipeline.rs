//! The whole pipeline over three generated fixtures, one of each media kind,
//! into a temporary database.

use std::path::{Path, PathBuf};
use std::process::Command;

use file_buddy::config::Weights;
use file_buddy::db::{Db, files, tags};
use file_buddy::ffmpeg::{self, Tool, thumbnail};
use file_buddy::index::jobs::Stage;
use file_buddy::index::{self, scan};
use file_buddy::media::Kind;
use file_buddy::search;

const VIDEO: &str = "my honest reaction.mp4";
const ANIMATION: &str = "silky anteater.gif";
const STILL: &str = "krispy kreme screenshot.png";

fn ffmpeg(args: &[&str]) {
    let output = Command::new(ffmpeg::path_of(Tool::Ffmpeg))
        .args(["-nostdin", "-v", "error", "-y"])
        .args(args)
        .output()
        .expect("run ffmpeg");
    assert!(
        output.status.success(),
        "ffmpeg failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixtures(dir: &Path) {
    let video = dir.join(VIDEO);
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=320x240:rate=10:duration=2",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=2",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-shortest",
        video.to_str().unwrap(),
    ]);
    let animation = dir.join(ANIMATION);
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=64x64:rate=10:duration=1",
        animation.to_str().unwrap(),
    ]);
    let still = dir.join(STILL);
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=red:size=64x64:d=1",
        "-frames:v",
        "1",
        still.to_str().unwrap(),
    ]);
}

struct Harness {
    _temp: tempfile::TempDir,
    clips: PathBuf,
    db_path: PathBuf,
    cache: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let clips = temp.path().join("clips");
        std::fs::create_dir_all(&clips).unwrap();
        fixtures(&clips);
        Self {
            db_path: temp.path().join("file-buddy.db"),
            cache: temp.path().join("thumbnails"),
            clips,
            _temp: temp,
        }
    }

    fn scan(&self) -> scan::ScanStats {
        let mut db = Db::open(&self.db_path).unwrap();
        let ignore = globset::GlobSetBuilder::new().build().unwrap();
        let stats = scan::scan(
            &mut db,
            std::slice::from_ref(&self.clips),
            &ignore,
            &self.cache,
        )
        .unwrap();
        drop(db);
        index::run(&self.db_path, &self.cache).unwrap();
        stats
    }

    fn transcribe(&self) {
        index::transcribe(&self.db_path, &self.cache).unwrap();
    }

    fn job(&self, path: &Path, stage: Stage) -> (String, i64) {
        let db = self.db();
        let row = files::by_path(db.conn(), path).unwrap().unwrap();
        db.conn()
            .query_row(
                "SELECT state, attempts FROM jobs WHERE hash = ?1 AND stage = ?2",
                file_buddy::rusqlite::params![&row.hash, stage.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    fn db(&self) -> Db {
        Db::open(&self.db_path).unwrap()
    }

    fn find(&self, query: &str) -> Vec<String> {
        let db = self.db();
        search::search(
            db.conn(),
            &search::Request::new(query).limit(10),
            &Weights::default(),
        )
        .unwrap()
        .into_iter()
        .map(|hit| hit.file.filename)
        .collect()
    }
}

#[test]
fn a_scan_fills_the_database_and_writes_thumbnails() {
    let harness = Harness::new();
    let stats = harness.scan();
    assert_eq!(stats.added, 3, "{stats:?}");
    assert_eq!(stats.unreadable, 0);

    let db = harness.db();
    let rows = files::all(db.conn()).unwrap();
    assert_eq!(rows.len(), 3);

    for row in &rows {
        assert_eq!(row.state, "ready", "{} is {}", row.filename, row.state);
        assert!(
            thumbnail::exists(&row.hash, &harness.cache),
            "no thumbnail for {}",
            row.filename
        );
    }

    let video = rows.iter().find(|r| r.filename == VIDEO).unwrap();
    assert_eq!(video.kind, Kind::Video);
    assert!(
        (video.duration.unwrap() - 2.0).abs() < 0.3,
        "{:?}",
        video.duration
    );

    let animation = rows.iter().find(|r| r.filename == ANIMATION).unwrap();
    assert_eq!(animation.kind, Kind::Animated);

    let still = rows.iter().find(|r| r.filename == STILL).unwrap();
    assert_eq!(still.kind, Kind::Still);
    assert_eq!(still.duration, None);
}

#[test]
fn an_fts_query_returns_the_right_file_by_filename() {
    let harness = Harness::new();
    harness.scan();

    assert_eq!(harness.find("honest"), vec![VIDEO]);
    assert_eq!(harness.find("anteat"), vec![ANIMATION]);
    assert_eq!(harness.find("krispy kreme"), vec![STILL]);
    assert!(harness.find("nothing here").is_empty());
    // The query is per keystroke, so a part of a word must match.
    assert_eq!(harness.find("rea"), vec![VIDEO]);
}

#[test]
fn a_tag_outranks_a_filename_and_the_tag_operator_limits_the_search() {
    let harness = Harness::new();
    harness.scan();
    let db = harness.db();
    let still = files::all(db.conn())
        .unwrap()
        .into_iter()
        .find(|r| r.filename == STILL)
        .unwrap();
    tags::attach(db.conn(), &still.hash, "Honest").unwrap();

    let hits = search::search(
        db.conn(),
        &search::Request::new("honest"),
        &Weights::default(),
    )
    .unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].file.filename, STILL);
    assert_eq!(hits[0].signal, search::Signal::Tags);

    let only_tagged = search::search(
        db.conn(),
        &search::Request::new("tag:honest"),
        &Weights::default(),
    )
    .unwrap();
    assert_eq!(only_tagged.len(), 1);
    assert_eq!(only_tagged[0].file.filename, STILL);
}

#[test]
fn a_chip_limits_the_results_to_one_kind() {
    let harness = Harness::new();
    harness.scan();
    let db = harness.db();

    let stills = search::search(
        db.conn(),
        &search::Request::new("").kinds(vec![Kind::Still]),
        &Weights::default(),
    )
    .unwrap();
    assert_eq!(
        stills
            .iter()
            .map(|h| h.file.filename.as_str())
            .collect::<Vec<_>>(),
        vec![STILL]
    );

    let moving = search::search(
        db.conn(),
        &search::Request::new("a").kinds(vec![Kind::Video, Kind::Animated]),
        &Weights::default(),
    )
    .unwrap();
    assert!(moving.iter().all(|h| h.file.kind != Kind::Still));
    assert!(!moving.is_empty());
}

#[test]
fn a_rename_keeps_the_hash_the_tags_and_the_finished_stages() {
    let harness = Harness::new();
    harness.scan();
    let hash_before;
    {
        let db = harness.db();
        let row = files::by_path(db.conn(), &harness.clips.join(VIDEO))
            .unwrap()
            .unwrap();
        hash_before = row.hash.clone();
        tags::attach(db.conn(), &row.hash, "reaction").unwrap();
    }

    let renamed = harness.clips.join("a few small beers.mp4");
    std::fs::rename(harness.clips.join(VIDEO), &renamed).unwrap();
    let stats = harness.scan();
    assert_eq!(stats.added, 1);
    assert_eq!(stats.removed, 1);

    let db = harness.db();
    let row = files::by_path(db.conn(), &renamed).unwrap().unwrap();
    assert_eq!(row.hash, hash_before);
    assert_eq!(row.state, "ready");
    assert_eq!(
        tags::of_hash(db.conn(), &row.hash).unwrap(),
        vec!["reaction"]
    );

    // No expensive stage ran again: the probe still shows one attempt.
    let attempts: i64 = db
        .conn()
        .query_row(
            "SELECT max(attempts) FROM jobs WHERE hash = ?1 AND stage = 'probe'",
            [&row.hash],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 1);

    assert_eq!(harness.find("beers"), vec!["a few small beers.mp4"]);
    assert!(harness.find("honest").is_empty());
}

#[test]
fn a_file_that_ffmpeg_cannot_open_records_the_error_and_stops() {
    let harness = Harness::new();
    std::fs::write(harness.clips.join("broken.mp4"), b"this is not a video").unwrap();
    harness.scan();

    let db = harness.db();
    let row = files::by_path(db.conn(), &harness.clips.join("broken.mp4"))
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "failed");

    let (state, error): (String, Option<String>) = db
        .conn()
        .query_row(
            "SELECT state, error FROM jobs WHERE hash = ?1 AND stage = 'probe'",
            [&row.hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "failed");
    assert!(error.unwrap().contains("ffprobe"));

    // It is still findable by name, and a rescan does not try it again.
    assert_eq!(harness.find("broken"), vec!["broken.mp4"]);
    let after = harness.scan();
    assert_eq!(after.unchanged, 4);
}

#[test]
fn a_long_file_skips_the_expensive_stages() {
    let harness = Harness::new();
    let long = harness.clips.join("a very long talk.mp4");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=64x64:rate=1:duration=360",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        long.to_str().unwrap(),
    ]);
    harness.scan();

    let db = harness.db();
    let row = files::by_path(db.conn(), &long).unwrap().unwrap();
    assert!(row.duration.unwrap() > 300.0, "{:?}", row.duration);

    for stage in [Stage::Whisper, Stage::Ocr] {
        let state: String = db
            .conn()
            .query_row(
                "SELECT state FROM jobs WHERE hash = ?1 AND stage = ?2",
                file_buddy::rusqlite::params![&row.hash, stage.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "skipped", "{}", stage.as_str());
    }
    // It still has a thumbnail and is still findable by name.
    assert!(thumbnail::exists(&row.hash, &harness.cache));
    assert_eq!(harness.find("very long talk"), vec!["a very long talk.mp4"]);
}

#[test]
fn speech_is_transcribed_once_and_found() {
    let harness = Harness::new();
    let speech = harness.clips.join("fox clip.mp4");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=160x120:rate=10:duration=4",
        "-f",
        "lavfi",
        "-i",
        "flite=text='the quick brown fox jumps over the lazy dog'",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-shortest",
        speech.to_str().unwrap(),
    ]);
    harness.scan();
    harness.transcribe();

    assert_eq!(harness.job(&speech, Stage::Whisper), ("done".into(), 1));
    assert_eq!(harness.find("brown fox"), vec!["fox clip.mp4"]);

    harness.scan();
    harness.transcribe();
    assert_eq!(harness.job(&speech, Stage::Whisper), ("done".into(), 1));
}

#[test]
fn a_video_with_no_audio_track_skips_whisper() {
    let harness = Harness::new();
    let silent = harness.clips.join("silent.mp4");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=64x64:rate=10:duration=1",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        silent.to_str().unwrap(),
    ]);
    harness.scan();
    assert_eq!(harness.job(&silent, Stage::Whisper).0, "skipped");
}
