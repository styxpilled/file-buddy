//! The whole pipeline over three generated fixtures, one of each media kind,
//! into a temporary database.

use std::path::{Path, PathBuf};
use std::process::Command;

use file_buddy::config::Weights;
use file_buddy::db::files::State;
use file_buddy::db::{Db, files, tags};
use file_buddy::ffmpeg::{self, Tool, thumbnail};
use file_buddy::index::jobs::Stage;
use file_buddy::index::{self, scan, watch};
use file_buddy::media::Kind;
use file_buddy::search;
use file_buddy::search::folder::{self, Entry, Sort, Stray};

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

    fn read_text(&self) {
        index::read_text(&self.db_path, &self.cache).unwrap();
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

    /// Index one batch of paths the way the watcher does.
    fn apply(&self, paths: &[PathBuf]) -> watch::WatchStats {
        let mut db = Db::open(&self.db_path).unwrap();
        let ignore = globset::GlobSetBuilder::new().build().unwrap();
        let (stats, retry) = watch::apply(&mut db, paths, &ignore, &self.cache).unwrap();
        assert!(retry.is_empty(), "{retry:?}");
        drop(db);
        index::run(&self.db_path, &self.cache).unwrap();
        stats
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
        assert_eq!(
            row.state,
            State::Ready,
            "{} is {:?}",
            row.filename,
            row.state
        );
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
    let mut db = harness.db();
    let still = files::all(db.conn())
        .unwrap()
        .into_iter()
        .find(|r| r.filename == STILL)
        .unwrap();
    tags::attach(db.conn_mut(), &[still.hash], "Honest").unwrap();

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
        let mut db = harness.db();
        let row = files::by_path(db.conn(), &harness.clips.join(VIDEO))
            .unwrap()
            .unwrap();
        hash_before = row.hash.clone();
        tags::attach(db.conn_mut(), &[row.hash], "reaction").unwrap();
    }

    let renamed = harness.clips.join("a few small beers.mp4");
    std::fs::rename(harness.clips.join(VIDEO), &renamed).unwrap();
    let stats = harness.scan();
    assert_eq!(stats.added, 1);
    assert_eq!(stats.missing, 1);

    let db = harness.db();
    let row = files::by_path(db.conn(), &renamed).unwrap().unwrap();
    assert_eq!(row.hash, hash_before);
    assert_eq!(row.state, State::Ready);
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
    assert_eq!(row.state, State::Failed);

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

#[test]
fn on_screen_text_is_read_once_and_found() {
    let harness = Harness::new();
    let captioned = harness.clips.join("caption clip.mp4");
    // White on black at a readable size. The recognizer is good at an overlay
    // caption and poor at small script, which is what issue 01 measured.
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=black:size=640x360:rate=10:duration=4",
        "-vf",
        r"drawtext=fontfile='C\:/Windows/Fonts/arial.ttf':text='the marmalade badger':fontcolor=white:fontsize=48:x=40:y=150",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        captioned.to_str().unwrap(),
    ]);
    harness.scan();
    harness.read_text();

    assert_eq!(harness.job(&captioned, Stage::Ocr), ("done".into(), 1));
    let db = harness.db();
    let row = files::by_path(db.conn(), &captioned).unwrap().unwrap();
    let lines = file_buddy::db::ocr::of_hash(db.conn(), &row.hash).unwrap();
    // Ten frames of the same caption give one line.
    assert_eq!(lines.len(), 1, "{lines:?}");
    drop(db);

    assert_eq!(harness.find("marmalade badger"), vec!["caption clip.mp4"]);

    // A rescan does not read it again.
    harness.scan();
    harness.read_text();
    assert_eq!(harness.job(&captioned, Stage::Ocr), ("done".into(), 1));
}

#[test]
fn a_still_with_no_text_is_read_and_records_nothing() {
    let harness = Harness::new();
    harness.scan();
    harness.read_text();

    let still = harness.clips.join(STILL);
    assert_eq!(harness.job(&still, Stage::Ocr), ("done".into(), 1));
    let db = harness.db();
    let row = files::by_path(db.conn(), &still).unwrap().unwrap();
    assert!(
        file_buddy::db::ocr::of_hash(db.conn(), &row.hash)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_deleted_file_keeps_its_tags_until_it_comes_back() {
    let harness = Harness::new();
    harness.scan();
    let still = harness.clips.join(STILL);
    {
        let mut db = harness.db();
        let row = files::by_path(db.conn(), &still).unwrap().unwrap();
        tags::attach(db.conn_mut(), &[row.hash], "doughnut").unwrap();
    }
    assert_eq!(harness.find("doughnut").len(), 1);

    let bytes = std::fs::read(&still).unwrap();
    std::fs::remove_file(&still).unwrap();
    let stats = harness.scan();
    assert_eq!(stats.missing, 1);
    assert!(harness.find("doughnut").is_empty());
    assert_eq!(files::all(harness.db().conn()).unwrap().len(), 2);

    std::fs::write(&still, &bytes).unwrap();
    let stats = harness.scan();
    assert_eq!(stats.missing, 0);
    assert_eq!(stats.added, 0, "the row came back, it was not added again");
    assert_eq!(harness.find("doughnut").len(), 1);
    assert_eq!(harness.job(&still, Stage::Probe).1, 1, "the probe ran once");
}

#[test]
fn the_watcher_indexes_a_new_file() {
    let harness = Harness::new();
    harness.scan();
    let new = harness.clips.join("a few small beers.mp4");
    std::fs::copy(harness.clips.join(VIDEO), &new).unwrap();

    let stats = harness.apply(std::slice::from_ref(&new));

    assert_eq!(stats.added, 1);
    let db = harness.db();
    let row = files::by_path(db.conn(), &new).unwrap().unwrap();
    assert_eq!(row.state, State::Ready, "the copy inherited the probe");
    assert_eq!(harness.find("beers"), vec!["a few small beers.mp4"]);
}

#[test]
fn the_watcher_follows_a_move_by_the_bytes() {
    let harness = Harness::new();
    harness.scan();
    let from = harness.clips.join(VIDEO);
    let into = harness.clips.join("moved");
    std::fs::create_dir(&into).unwrap();
    let to = into.join(VIDEO);
    let hash_before = files::by_path(harness.db().conn(), &from)
        .unwrap()
        .unwrap()
        .hash;
    std::fs::rename(&from, &to).unwrap();

    let stats = harness.apply(&[from.clone(), to.clone()]);

    assert_eq!(stats.moved, 1, "{stats:?}");
    assert_eq!(stats.added, 0);
    assert_eq!(stats.missing, 0);
    let db = harness.db();
    assert!(files::by_path(db.conn(), &from).unwrap().is_none());
    assert_eq!(
        files::by_path(db.conn(), &to).unwrap().unwrap().hash,
        hash_before
    );
    assert_eq!(files::all(db.conn()).unwrap().len(), 3);
    assert_eq!(harness.job(&to, Stage::Probe).1, 1, "nothing ran again");
}

#[test]
fn the_watcher_marks_a_deleted_folder_missing() {
    let harness = Harness::new();
    let inner = harness.clips.join("inner");
    std::fs::create_dir(&inner).unwrap();
    std::fs::copy(harness.clips.join(STILL), inner.join(STILL)).unwrap();
    harness.scan();
    assert_eq!(files::all(harness.db().conn()).unwrap().len(), 4);

    std::fs::remove_dir_all(&inner).unwrap();
    let stats = harness.apply(std::slice::from_ref(&inner));

    // The copy shares its bytes with the still that stays, so its row is
    // redundant and goes.
    assert_eq!(stats.missing, 1);
    let db = harness.db();
    assert!(
        files::by_path(db.conn(), &inner.join(STILL))
            .unwrap()
            .is_none()
    );
    assert_eq!(files::all(db.conn()).unwrap().len(), 3);
}

#[test]
fn the_watcher_reports_what_changed_on_disk() {
    let harness = Harness::new();
    harness.scan();
    let ignore = globset::GlobSetBuilder::new().build().unwrap();
    let watcher = watch::watch(std::slice::from_ref(&harness.clips), &ignore).unwrap();

    let new = harness.clips.join("a few small beers.mp4");
    std::fs::copy(harness.clips.join(VIDEO), &new).unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let batch = loop {
        if let Some(batch) = watcher.batch() {
            break batch;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the watcher said nothing"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert!(batch.contains(&new), "{batch:?}");
}

#[test]
fn a_folder_lists_its_folders_then_its_files_then_what_the_search_cannot_find() {
    let harness = Harness::new();
    let memes = harness.clips.join("memes");
    std::fs::create_dir(&memes).unwrap();
    std::fs::copy(harness.clips.join(STILL), memes.join("krispy copy.png")).unwrap();
    harness.scan();
    std::fs::write(harness.clips.join("notes.txt"), "not a clip").unwrap();
    std::fs::copy(
        harness.clips.join(ANIMATION),
        harness.clips.join("late.gif"),
    )
    .unwrap();
    std::fs::copy(harness.clips.join(STILL), harness.clips.join("secret.png")).unwrap();

    let db = harness.db();
    assert_eq!(
        files::in_folder(db.conn(), &harness.clips).unwrap().len(),
        3
    );
    let ignore = globset::GlobSetBuilder::new()
        .add(globset::Glob::new("**/secret*").unwrap())
        .build()
        .unwrap();
    let entries = folder::list(db.conn(), &harness.clips, &ignore, &[], Sort::Name).unwrap();
    let listed: Vec<(String, &str)> = entries
        .iter()
        .map(|entry| {
            let name = files::FileRow::name_of(entry.path());
            let what = match entry {
                Entry::Folder(_) => "folder",
                Entry::File(_) => "file",
                Entry::Stray { why, .. } => why.as_str(),
            };
            (name, what)
        })
        .collect();
    let expected = [
        ("memes", "folder"),
        (STILL, "file"),
        (VIDEO, "file"),
        (ANIMATION, "file"),
        ("late.gif", Stray::Unindexed.as_str()),
        ("secret.png", Stray::Ignored.as_str()),
        ("notes.txt", Stray::Unmatched.as_str()),
    ];
    let expected: Vec<(String, &str)> = expected
        .into_iter()
        .map(|(name, what)| (name.to_string(), what))
        .collect();
    assert_eq!(listed, expected);

    // The chips limit the indexed files only.
    let stills = folder::list(
        db.conn(),
        &harness.clips,
        &ignore,
        &[Kind::Still],
        Sort::Name,
    )
    .unwrap();
    assert_eq!(stills.iter().filter_map(Entry::hit).count(), 1);
    assert_eq!(stills.len(), 5);
}

#[test]
fn a_folder_sorts_its_files_by_name_time_or_size_and_keeps_the_groups() {
    let harness = Harness::new();
    let day = std::time::Duration::from_secs(86_400);
    let now = std::time::SystemTime::now();
    for (name, age) in [(VIDEO, 3), (STILL, 2), (ANIMATION, 1)] {
        let file = std::fs::File::options()
            .write(true)
            .open(harness.clips.join(name))
            .unwrap();
        file.set_modified(now - day * age).unwrap();
    }
    harness.scan();
    std::fs::write(harness.clips.join("a note.txt"), "not a clip").unwrap();
    std::fs::create_dir(harness.clips.join("zz")).unwrap();

    let db = harness.db();
    let ignore = globset::GlobSetBuilder::new().build().unwrap();
    let names = |sort: Sort| -> Vec<String> {
        folder::list(db.conn(), &harness.clips, &ignore, &[], sort)
            .unwrap()
            .iter()
            .map(|entry| files::FileRow::name_of(entry.path()))
            .collect()
    };
    let expected = |files: [&str; 3]| -> Vec<String> {
        ["zz", files[0], files[1], files[2], "a note.txt"]
            .map(str::to_string)
            .to_vec()
    };
    assert_eq!(names(Sort::Name), expected([STILL, VIDEO, ANIMATION]));
    assert_eq!(names(Sort::Recent), expected([ANIMATION, STILL, VIDEO]));

    let by_size = folder::list(db.conn(), &harness.clips, &ignore, &[], Sort::Size).unwrap();
    let sizes: Vec<u64> = by_size
        .iter()
        .filter_map(Entry::hit)
        .map(|hit| hit.file.size)
        .collect();
    assert_eq!(sizes.len(), 3);
    assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]), "{sizes:?}");
    assert!(matches!(
        by_size.last(),
        Some(Entry::Stray { size: 10, .. })
    ));
}

#[test]
fn a_search_in_a_folder_finds_only_the_files_under_it() {
    let harness = Harness::new();
    let memes = harness.clips.join("memes");
    std::fs::create_dir(&memes).unwrap();
    std::fs::copy(harness.clips.join(STILL), memes.join("krispy copy.png")).unwrap();
    harness.scan();

    let db = harness.db();
    let find = |within: Option<PathBuf>| -> Vec<String> {
        let request = search::Request::new("krispy").within(within);
        let mut names: Vec<String> = search::search(db.conn(), &request, &Weights::default())
            .unwrap()
            .into_iter()
            .map(|hit| hit.file.filename)
            .collect();
        names.sort();
        names
    };
    assert_eq!(find(Some(memes)), vec!["krispy copy.png"]);
    assert_eq!(find(None), vec!["krispy copy.png", STILL]);
}
