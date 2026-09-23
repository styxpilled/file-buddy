//! The search index. One row per file, four columns, rebuilt from the tables
//! that own the text. Nothing in `fts` is a source of truth.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::media::filename;
use crate::ocr::noise;

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
    // The `ocr` table keeps every line the recognizer read, one per line. The
    // index takes the lines worth searching and holds them on one line, because
    // a snippet is shown in a single row of a tile.
    let read: Option<String> = conn
        .query_row("SELECT text FROM ocr WHERE hash = ?1", [&hash], |row| {
            row.get(0)
        })
        .optional()?;
    let ocr = read
        .unwrap_or_default()
        .lines()
        .filter(|line| !noise::is_noise(line))
        .collect::<Vec<_>>()
        .join(" ");

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

/// Whether the file has an index row.
pub fn has_row(conn: &Connection, file_id: i64) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM fts WHERE rowid = ?1", [file_id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// Rebuild every index row, and return how many. This is what a change to
/// `ocr::noise`, to the filename split or to a weight of the index costs: the
/// tables that own the text are untouched.
pub fn rebuild(conn: &Connection) -> Result<usize> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM files")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for id in &ids {
        refresh(conn, *id)?;
    }
    Ok(ids.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, ocr};

    fn seeded() -> Db {
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

    fn indexed_ocr(db: &Db) -> String {
        db.conn()
            .query_row("SELECT ocr FROM fts", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn the_watermark_is_left_out_of_the_index_but_stays_in_the_table() {
        let db = seeded();
        ocr::replace(
            db.conn(),
            "h",
            &["TikTok".into(), "@ mangoslug".into(), "the caption".into()],
        )
        .unwrap();

        refresh_hash(db.conn(), "h").unwrap();

        assert_eq!(indexed_ocr(&db), "the caption");
        // The rule may change, and rereading every file to change it is the
        // cost this avoids.
        assert_eq!(
            ocr::of_hash(db.conn(), "h").unwrap(),
            ["TikTok", "@ mangoslug", "the caption"]
        );
    }

    #[test]
    fn the_index_holds_the_ocr_lines_on_one_line() {
        let db = seeded();
        ocr::replace(db.conn(), "h", &["first line".into(), "second line".into()]).unwrap();

        refresh_hash(db.conn(), "h").unwrap();

        assert_eq!(indexed_ocr(&db), "first line second line");
    }
}
