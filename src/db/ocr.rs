//! The on-screen text of a file, keyed by content hash. One row holds every
//! line, and the `ocr` column of `fts` is its text.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

/// Replace the text of these bytes. No lines is still a row: it is the record
/// that the file was read and holds nothing.
pub fn replace(conn: &Connection, hash: &str, lines: &[String]) -> Result<()> {
    conn.execute(
        "INSERT INTO ocr (hash, text) VALUES (?1, ?2)
         ON CONFLICT (hash) DO UPDATE SET text = ?2",
        params![hash, lines.join("\n")],
    )?;
    Ok(())
}

pub fn of_hash(conn: &Connection, hash: &str) -> Result<Vec<String>> {
    let text: Option<String> = conn
        .query_row("SELECT text FROM ocr WHERE hash = ?1", [hash], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(text
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect())
}
