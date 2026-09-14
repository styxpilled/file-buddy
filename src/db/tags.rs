//! Tags are the one kind of data the application cannot rebuild from the files.
//! They belong to the content, so a rename keeps them.

use anyhow::Result;
use rusqlite::{Connection, params};

use crate::db::fts;

/// The tag id, creating the tag if it is new. Tags are lowercase.
pub fn ensure(conn: &Connection, name: &str) -> Result<i64> {
    let name = name.trim().to_lowercase();
    conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [&name])?;
    Ok(
        conn.query_row("SELECT id FROM tags WHERE name = ?1", [&name], |row| {
            row.get(0)
        })?,
    )
}

pub fn attach(conn: &Connection, hash: &str, name: &str) -> Result<()> {
    let tag_id = ensure(conn, name)?;
    conn.execute(
        "INSERT OR IGNORE INTO file_tags (hash, tag_id) VALUES (?1, ?2)",
        params![hash, tag_id],
    )?;
    fts::refresh_hash(conn, hash)
}

pub fn detach(conn: &Connection, hash: &str, name: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM file_tags WHERE hash = ?1
         AND tag_id IN (SELECT id FROM tags WHERE name = ?2)",
        params![hash, name.trim().to_lowercase()],
    )?;
    fts::refresh_hash(conn, hash)
}

pub fn of_hash(conn: &Connection, hash: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT t.name FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
         WHERE ft.hash = ?1 ORDER BY t.name",
    )?;
    let rows = statement.query_map([hash], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn all(conn: &Connection) -> Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT name FROM tags ORDER BY name")?;
    let rows = statement.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}
