//! A headless front end for the index, so the pipeline can be run and checked
//! without the user interface.
//!
//!     fb scan            walk the roots, then run every stage that has a worker,
//!                        text and speech last
//!     fb search <query>  print the ranked results
//!     fb reindex         rebuild the search index from the tables that own
//!                        the text, after a rule that shapes it changed
//!     fb status          counts per state and per stage
//!     fb worker whisper  the speech worker process, which the pipeline starts

use std::sync::atomic::AtomicBool;

use anyhow::Result;
use file_buddy::config::Config;
use file_buddy::db::{Db, files, fts};
use file_buddy::index::jobs::{self, Stage};
use file_buddy::index::{self, scan};
use file_buddy::{paths, search, speech};

fn main() -> Result<()> {
    env_logger::init();

    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "status".into());
    if command == "worker" {
        return match args.next().as_deref() {
            Some("whisper") => speech::serve(),
            other => anyhow::bail!("unknown worker {other:?}. Use whisper"),
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
            let query = args.collect::<Vec<_>>().join(" ");
            let db = Db::open(&db_path)?;
            let request = search::Request::new(query).limit(50);
            for hit in search::search(db.conn(), &request, &config.weights)? {
                println!(
                    "{:>8.3}  {:<12} {}",
                    hit.score,
                    hit.signal.as_str(),
                    hit.file.path.display()
                );
            }
        }
        "status" => {
            let db = Db::open(&db_path)?;
            println!("database: {}", db_path.display());
            println!("files: {}", files::all(db.conn())?.len());
            for stage in [Stage::Probe, Stage::Thumbnail, Stage::Whisper, Stage::Ocr] {
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
            anyhow::bail!("unknown command {other:?}. Use scan, search, reindex or status");
        }
    }
    Ok(())
}
