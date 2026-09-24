//! Speech segments, keyed by content hash. The `speech` column of `fts` is
//! their joined text.

use anyhow::Result;
use rusqlite::{Connection, params};

use crate::db::edits::{self, Table};
use crate::speech::Segment;

/// Replace every segment of these bytes, unless the user edited one.
pub fn replace(conn: &Connection, hash: &str, segments: &[Segment]) -> Result<()> {
    let transaction = conn.unchecked_transaction()?;
    if edits::any(&transaction, Table::Segments, hash)? {
        return Ok(());
    }
    transaction.execute("DELETE FROM segments WHERE hash = ?1", [hash])?;
    {
        let mut insert = transaction.prepare(
            "INSERT INTO segments (hash, start_ms, end_ms, text, confidence, no_speech_prob)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for segment in segments {
            insert.execute(params![
                hash,
                segment.start_ms,
                segment.end_ms,
                segment.text,
                segment.confidence,
                segment.no_speech_prob
            ])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// A deleted segment is not one.
pub fn of_hash(conn: &Connection, hash: &str) -> Result<Vec<Segment>> {
    let mut statement = conn.prepare(
        "SELECT start_ms, end_ms, text, confidence, no_speech_prob FROM segments
         WHERE hash = ?1 AND text <> '' ORDER BY start_ms, id",
    )?;
    let rows = statement.query_map([hash], |row| {
        Ok(Segment {
            start_ms: row.get(0)?,
            end_ms: row.get(1)?,
            text: row.get(2)?,
            confidence: row.get(3)?,
            no_speech_prob: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
