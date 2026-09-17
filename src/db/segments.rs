//! Speech segments, keyed by content hash. The `speech` column of `fts` is
//! their joined text.

use anyhow::Result;
use rusqlite::{Connection, params};

use crate::ffmpeg::whisper::Segment;

/// Replace every segment of these bytes. The filter gives no confidence, so the
/// column stays NULL.
pub fn replace(conn: &Connection, hash: &str, segments: &[Segment]) -> Result<()> {
    let transaction = conn.unchecked_transaction()?;
    transaction.execute("DELETE FROM segments WHERE hash = ?1", [hash])?;
    {
        let mut insert = transaction.prepare(
            "INSERT INTO segments (hash, start_ms, end_ms, text) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for segment in segments {
            insert.execute(params![
                hash,
                segment.start_ms,
                segment.end_ms,
                segment.text
            ])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn of_hash(conn: &Connection, hash: &str) -> Result<Vec<Segment>> {
    let mut statement = conn
        .prepare("SELECT start_ms, end_ms, text FROM segments WHERE hash = ?1 ORDER BY start_ms")?;
    let rows = statement.query_map([hash], |row| {
        Ok(Segment {
            start_ms: row.get(0)?,
            end_ms: row.get(1)?,
            text: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
