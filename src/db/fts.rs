//! The search index. One row per file, four columns, rebuilt from the tables
//! that own the text. Nothing in `fts` is a source of truth.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::media::filename;

/// Rebuild the index row of one file.
pub fn refresh(conn: &Connection, file_id: i64) -> Result<()> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT filename, hash FROM files WHERE id = ?1",
            [file_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((filename, hash)) = row else {
        conn.execute("DELETE FROM fts WHERE rowid = ?1", [file_id])?;
        return Ok(());
    };

    let tags: String = conn.query_row(
        "SELECT coalesce(group_concat(t.name, ' '), '')
         FROM file_tags ft JOIN tags t ON t.id = ft.tag_id WHERE ft.hash = ?1",
        [&hash],
        |row| row.get(0),
    )?;
    let speech: String = conn.query_row(
        "SELECT coalesce(group_concat(text, ' '), '') FROM segments WHERE hash = ?1",
        [&hash],
        |row| row.get(0),
    )?;
    let ocr: String = conn
        .query_row("SELECT text FROM ocr WHERE hash = ?1", [&hash], |row| {
            row.get(0)
        })
        .optional()?
        .unwrap_or_default();

    conn.execute("DELETE FROM fts WHERE rowid = ?1", [file_id])?;
    conn.execute(
        "INSERT INTO fts (rowid, tags, filename, speech, ocr) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![file_id, tags, filename::index_form(&filename), speech, ocr],
    )?;
    Ok(())
}

/// Rebuild the index row of every file that holds these bytes, after a stage
/// produced new text for them.
pub fn refresh_hash(conn: &Connection, hash: &str) -> Result<()> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM files WHERE hash = ?1")?
        .query_map([hash], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for id in ids {
        refresh(conn, id)?;
    }
    Ok(())
}
