//! The on-screen text of a file, keyed by content hash. One row per line, with
//! the time of the first frame that shows it. The `ocr` column of `fts` is the
//! lines the index keeps.

use anyhow::Result;
use rusqlite::{Connection, params};

use crate::db::edits::{self, Table};
use crate::ocr::noise;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// `None` for a line read before lines had times.
    pub time_ms: Option<i64>,
    pub text: String,
    /// The user changed or added the line.
    pub edited: bool,
}

impl Line {
    pub fn new(time_ms: i64, text: impl Into<String>) -> Self {
        Self {
            time_ms: Some(time_ms),
            text: text.into(),
            edited: false,
        }
    }
}

/// Whether the index takes a line. A line the user typed is always taken, even
/// when the watermark rule would leave it out.
pub fn indexed(text: &str, edited: bool) -> bool {
    !text.is_empty() && (edited || !noise::is_noise(text))
}

/// Replace every line of these bytes, unless the user edited one. The job
/// records that the file was read, so a file with no text has no rows.
pub fn replace(conn: &Connection, hash: &str, lines: &[Line]) -> Result<()> {
    let transaction = conn.unchecked_transaction()?;
    if edits::any(&transaction, Table::Ocr, hash)? {
        return Ok(());
    }
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
/// A deleted line is not one.
pub fn of_hash(conn: &Connection, hash: &str) -> Result<Vec<Line>> {
    let mut statement = conn.prepare(
        "SELECT time_ms, text, edited FROM ocr
         WHERE hash = ?1 AND text <> '' ORDER BY time_ms, id",
    )?;
    let rows = statement.query_map([hash], |row| {
        Ok(Line {
            time_ms: row.get(0)?,
            text: row.get(1)?,
            edited: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
