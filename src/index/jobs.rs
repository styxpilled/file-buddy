//! A durable job queue in SQLite. One row per content hash per stage, so a
//! failure in one stage does not discard the result of another.

use std::str::FromStr;

use anyhow::Result;
use rusqlite::{Connection, TransactionBehavior, params};

use crate::db::now;
use crate::media::Kind;

/// Two failed attempts, then the job records the error and stops.
pub const MAX_ATTEMPTS: i64 = 2;
/// Longer than this and the expensive stages do not run. The file stays
/// findable by name and appears in Review.
pub const LONG_FILE_SECONDS: f64 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Probe,
    Thumbnail,
    Whisper,
    Ocr,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Probe => "probe",
            Stage::Thumbnail => "thumbnail",
            Stage::Whisper => "whisper",
            Stage::Ocr => "ocr",
        }
    }

    /// How many of this stage may run at once.
    pub fn concurrency(self) -> usize {
        match self {
            Stage::Probe | Stage::Thumbnail => 8,
            Stage::Whisper => 2,
            Stage::Ocr => 4,
        }
    }

    /// The stages that run after a probe, for a file of this kind.
    pub fn after_probe(kind: Kind) -> &'static [Stage] {
        match kind {
            Kind::Video => &[Stage::Whisper, Stage::Ocr],
            Kind::Animated | Kind::Still => &[Stage::Ocr],
        }
    }
}

impl FromStr for Stage {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "probe" => Stage::Probe,
            "thumbnail" => Stage::Thumbnail,
            "whisper" => Stage::Whisper,
            "ocr" => Stage::Ocr,
            other => anyhow::bail!("unknown stage {other:?}"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub hash: String,
    pub stage: Stage,
    pub attempts: i64,
}

/// Queue a stage, unless the queue already holds it. A finished stage is never
/// queued again, which is what keeps a rename cheap.
pub fn enqueue(conn: &Connection, hash: &str, stage: Stage) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO jobs (hash, stage, state, updated_at) VALUES (?1, ?2, 'pending', ?3)",
        params![hash, stage.as_str(), now()],
    )?;
    Ok(())
}

/// Record a stage as not applicable to this file, with the reason.
pub fn skip(conn: &Connection, hash: &str, stage: Stage, reason: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO jobs (hash, stage, state, error, updated_at) VALUES (?1, ?2, 'skipped', ?3, ?4)
         ON CONFLICT (hash, stage) DO UPDATE SET state = 'skipped', error = ?3, updated_at = ?4",
        params![hash, stage.as_str(), reason, now()],
    )?;
    Ok(())
}

/// Take up to `limit` pending jobs of one stage and mark them running. The
/// attempt is counted here, so a worker that dies does not get a free retry.
pub fn claim(conn: &mut Connection, stage: Stage, limit: usize) -> Result<Vec<Job>> {
    // Immediate, not deferred. A deferred transaction that reads and then writes
    // fails at once with SQLITE_BUSY_SNAPSHOT instead of waiting its turn.
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let jobs: Vec<Job> = {
        let mut statement = transaction.prepare(
            "SELECT hash, attempts FROM jobs
             WHERE stage = ?1 AND state = 'pending' AND attempts < ?2
             ORDER BY updated_at LIMIT ?3",
        )?;
        let rows =
            statement.query_map(params![stage.as_str(), MAX_ATTEMPTS, limit as i64], |row| {
                Ok(Job {
                    hash: row.get(0)?,
                    stage,
                    attempts: row.get(1)?,
                })
            })?;
        rows.collect::<Result<_, _>>()?
    };
    {
        let mut update = transaction.prepare(
            "UPDATE jobs SET state = 'running', attempts = attempts + 1, updated_at = ?3
             WHERE hash = ?1 AND stage = ?2",
        )?;
        for job in &jobs {
            update.execute(params![job.hash, stage.as_str(), now()])?;
        }
    }
    transaction.commit()?;
    Ok(jobs)
}

pub fn finish(conn: &Connection, hash: &str, stage: Stage) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET state = 'done', error = NULL, updated_at = ?3 WHERE hash = ?1 AND stage = ?2",
        params![hash, stage.as_str(), now()],
    )?;
    Ok(())
}

/// Record a failure. The job stops after `MAX_ATTEMPTS` and keeps the error.
pub fn fail(conn: &Connection, hash: &str, stage: Stage, error: &str) -> Result<()> {
    conn.execute(
        "UPDATE jobs
         SET state = CASE WHEN attempts >= ?4 THEN 'failed' ELSE 'pending' END,
             error = ?3, updated_at = ?5
         WHERE hash = ?1 AND stage = ?2",
        params![hash, stage.as_str(), error, MAX_ATTEMPTS, now()],
    )?;
    Ok(())
}

/// Any job of this stage still waiting for a worker.
pub fn pending_count(conn: &Connection, stage: Stage) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM jobs WHERE stage = ?1 AND state = 'pending' AND attempts < ?2",
        params![stage.as_str(), MAX_ATTEMPTS],
        |row| row.get(0),
    )?)
}

/// Move every job left running by a process that died back to pending. Called
/// once at startup, never while workers run.
pub fn requeue_running(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE jobs SET state = 'pending', updated_at = ?1 WHERE state = 'running'",
        [now()],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO files (path, hash, filename, kind, size, mtime, seen_at)
                 VALUES ('a.mp4', 'h', 'a.mp4', 'video', 1, 0, 0)",
                [],
            )
            .unwrap();
        db
    }

    #[test]
    fn a_claimed_job_is_not_claimed_twice() {
        let mut db = db();
        enqueue(db.conn(), "h", Stage::Probe).unwrap();
        assert_eq!(claim(db.conn_mut(), Stage::Probe, 8).unwrap().len(), 1);
        assert!(claim(db.conn_mut(), Stage::Probe, 8).unwrap().is_empty());
    }

    #[test]
    fn two_failures_stop_the_job() {
        let mut db = db();
        enqueue(db.conn(), "h", Stage::Probe).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            let jobs = claim(db.conn_mut(), Stage::Probe, 8).unwrap();
            assert_eq!(jobs.len(), 1);
            fail(db.conn(), "h", Stage::Probe, "broken").unwrap();
        }
        assert!(claim(db.conn_mut(), Stage::Probe, 8).unwrap().is_empty());
        let (state, error): (String, String) = db
            .conn()
            .query_row(
                "SELECT state, error FROM jobs WHERE hash = 'h'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "failed");
        assert_eq!(error, "broken");
    }

    #[test]
    fn a_finished_stage_is_never_queued_again() {
        let mut db = db();
        enqueue(db.conn(), "h", Stage::Probe).unwrap();
        claim(db.conn_mut(), Stage::Probe, 8).unwrap();
        finish(db.conn(), "h", Stage::Probe).unwrap();
        enqueue(db.conn(), "h", Stage::Probe).unwrap();
        assert!(claim(db.conn_mut(), Stage::Probe, 8).unwrap().is_empty());
    }
}
