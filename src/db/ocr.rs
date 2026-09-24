//! The on-screen text of a file, keyed by content hash. One row per line, with
//! the time of the first frame that shows it. The `ocr` column of `fts` is the
//! lines the index keeps.

use anyhow::Result;
use rusqlite::{Connection, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// `None` for a line read before lines had times.
    pub time_ms: Option<i64>,
    pub text: String,
}

impl Line {
    pub fn new(time_ms: i64, text: impl Into<String>) -> Self {
        Self {
            time_ms: Some(time_ms),
            text: text.into(),
        }
    }
}

/// Replace every line of these bytes. The job records that the file was read,
/// so a file with no text has no rows.
pub fn replace(conn: &Connection, hash: &str, lines: &[Line]) -> Result<()> {
    let transaction = conn.unchecked_transaction()?;
    transaction.execute("DELETE FROM ocr WHERE hash = ?1", [hash])?;
    {
        let mut insert =
            transaction.prepare("INSERT INTO ocr (hash, time_ms, text) VALUES (?1, ?2, ?3)")?;
        for line in lines {
            insert.execute(params![hash, line.time_ms, line.text])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// In the order they show, and in the order they were read within a frame.
pub fn of_hash(conn: &Connection, hash: &str) -> Result<Vec<Line>> {
    let mut statement =
        conn.prepare("SELECT time_ms, text FROM ocr WHERE hash = ?1 ORDER BY time_ms, id")?;
    let rows = statement.query_map([hash], |row| {
        Ok(Line {
            time_ms: row.get(0)?,
            text: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
