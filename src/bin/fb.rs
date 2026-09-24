//! A headless front end for the index, so the pipeline can be run and checked
//! without the user interface.
//!
//!     fb scan            walk the roots, then run every stage that has a worker,
//!                        text and speech last
//!     fb search <query>  print the ranked results
//!     fb visual <query>  print the 20 files that look most like the query
//!     fb reindex         rebuild the search index from the tables that own
//!                        the text, after a rule that shapes it changed
//!     fb status          counts per state and per stage
//!     fb worker whisper  the speech worker process, which the pipeline starts
//!     fb worker clip     the image worker process, which the pipeline starts

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::Result;
use file_buddy::config::Config;
use file_buddy::db::{Db, files, fts};
use file_buddy::index::jobs::{self, Stage};
use file_buddy::index::{self, scan};
use file_buddy::search::query;
use file_buddy::search::visual::{self, Vectors};
use file_buddy::{clip, paths, search, speech};

fn main() -> Result<()> {
    env_logger::init();

    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "status".into());
    if command == "worker" {
        return match args.next().as_deref() {
            Some("whisper") => speech::serve(),
            Some("clip") => clip::image::serve(),
            other => anyhow::bail!("unknown worker {other:?}. Use whisper or clip"),
        };
    }

    let config = Config::load_or_create(&paths::config_path()?)?;
    let db_path = paths::database_path()?;
    let cache = paths::thumbnail_dir()?;

    match command.as_str() {
        "scan" => {
            let mut db = Db::open(&db_path)?;
            let stats = scan::scan(&mut db, &config.roots, &config.ignore_set()?, &cache)?;
            drop(db);
            println!(
                "scanned: {} added, {} changed, {} unchanged, {} missing, {} unreadable",
                stats.added, stats.changed, stats.unchanged, stats.missing, stats.unreadable
            );
            let stages = index::run(&db_path, &cache)?;
            println!(
                "probe: {} done, {} failed. thumbnail: {} done, {} failed",
                stages.probe.done,
                stages.probe.failed,
                stages.thumbnail.done,
                stages.thumbnail.failed
            );
            match index::read_text(&db_path, &cache, &AtomicBool::new(config.paused.ocr)) {
                Ok(ocr) => println!("ocr: {} done, {} failed", ocr.done, ocr.failed),
                Err(e) => println!("ocr: {e:#}"),
            }
            match index::embed_frames(&db_path, &cache, &AtomicBool::new(config.paused.clip)) {
                Ok(clip) => println!("clip: {} done, {} failed", clip.done, clip.failed),
                Err(e) => println!("clip: {e:#}"),
            }
            match index::transcribe(&db_path, &cache, &AtomicBool::new(config.paused.whisper)) {
                Ok(whisper) => {
                    println!("whisper: {} done, {} failed", whisper.done, whisper.failed)
                }
                Err(e) => println!("whisper: {e:#}"),
            }
        }
        "reindex" => {
            let db = Db::open(&db_path)?;
            println!("reindexed {} files", fts::rebuild(db.conn())?);
        }
        "search" => {
            let text = args.collect::<Vec<_>>().join(" ");
            let db = Db::open(&db_path)?;
            let request = search::Request::new(text.clone()).limit(50);
            let mut hits = search::search(db.conn(), &request, &config.weights)?;
            let visual_only = query::visual_only(&text).is_some();
            if let Some(words) = query::visual_text(&text)
                && (config.weights.visual > 0.0 || visual_only)
            {
                let scored = look(&db, &words)?;
                let found = visual::hits(db.conn(), &request, &scored)?;
                hits = visual::fuse(hits, found, config.weights.visual, request.limit);
            }
            for hit in hits {
                println!(
                    "{:>8.3}  {:<12} {}",
                    hit.score,
                    hit.signal.as_str(),
                    hit.file.path.display()
                );
            }
        }
        "visual" => {
            let text = args.collect::<Vec<_>>().join(" ");
            let db = Db::open(&db_path)?;
            let mut vectors = Vectors::default();
            vectors.refresh(db.conn())?;
            let mut encoder = clip::text::TextEncoder::load(&paths::clip_model()?)?;
            let started = Instant::now();
            let query = encoder.embed(&text)?;
            let embedded = started.elapsed();
            let all = vectors.search(&query, f32::MIN, f32::MAX);
            let searched = started.elapsed() - embedded;
            let kept = vectors.search(&query, visual::FLOOR, visual::DELTA).len();
            println!(
                "text encoder {embedded:.0?}, search {searched:.0?}. {kept} of {} kept (floor {}, delta {})",
                all.len(),
                visual::FLOOR,
                visual::DELTA
            );
            let paths: HashMap<String, PathBuf> = files::all(db.conn())?
                .into_iter()
                .map(|row| (row.hash, row.path))
                .collect();
            for (at, scored) in all.iter().enumerate().take(kept.max(20) + 5) {
                if at == kept {
                    println!("  ------ cutoff");
                }
                let path = paths.get(&scored.hash).map(|p| p.display().to_string());
                println!(
                    "{:>7.4}  {:>6.1}s  {}",
                    scored.score,
                    scored.time_ms as f64 / 1000.0,
                    path.unwrap_or_else(|| scored.hash.clone())
                );
            }
        }
        "status" => {
            let db = Db::open(&db_path)?;
            println!("database: {}", db_path.display());
            println!("files: {}", files::all(db.conn())?.len());
            for stage in [
                Stage::Probe,
                Stage::Thumbnail,
                Stage::Ocr,
                Stage::Clip,
                Stage::Whisper,
            ] {
                let summary = jobs::counts(db.conn(), stage)?
                    .iter()
                    .map(|(state, count)| format!("{count} {state}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let summary = if summary.is_empty() {
                    "-".into()
                } else {
                    summary
                };
                println!("{:<10} {summary}", stage.as_str());
            }
        }
        other => {
            anyhow::bail!("unknown command {other:?}. Use scan, search, visual, reindex or status");
        }
    }
    Ok(())
}

/// The files that look like `words`, best first, under the cutoff.
fn look(db: &Db, words: &str) -> Result<Vec<visual::Scored>> {
    let mut vectors = Vectors::default();
    vectors.refresh(db.conn())?;
    let query = clip::text::TextEncoder::load(&paths::clip_model()?)?.embed(words)?;
    Ok(vectors.search(&query, visual::FLOOR, visual::DELTA))
}
