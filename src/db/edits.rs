//! The speech segments and screen lines the user changed, deleted or added.
//! Like tags, the application cannot rebuild them from the files. A file with
//! an edited row keeps its rows when its stage runs again.

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db::fts;

/// The table a row lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Segments,
    Ocr,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Table::Segments => "segments",
            Table::Ocr => "ocr",
        }
    }

    fn time(self) -> &'static str {
        match self {
            Table::Segments => "start_ms",
            Table::Ocr => "time_ms",
        }
    }
}

/// A row as the side pane shows it. A deleted row is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: i64,
    /// `None` for a screen line read before lines had times.
    pub time_ms: Option<i64>,
    pub text: String,
    pub edited: bool,
}

/// The rows of these bytes, in the order they show.
pub fn rows(conn: &Connection, table: Table, hash: &str) -> Result<Vec<Row>> {
    let sql = format!(
        "SELECT id, {time}, text, edited FROM {name}
         WHERE hash = ?1 AND text <> '' ORDER BY {time}, id",
        name = table.name(),
        time = table.time(),
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([hash], |row| {
        Ok(Row {
            id: row.get(0)?,
            time_ms: row.get(1)?,
            text: row.get(2)?,
            edited: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Whether the user edited a row of these bytes. A new run of the stage then
/// leaves the rows as they are.
pub fn any(conn: &Connection, table: Table, hash: &str) -> Result<bool> {
    let sql = format!(
        "SELECT 1 FROM {} WHERE hash = ?1 AND edited LIMIT 1",
        table.name()
    );
    Ok(conn
        .query_row(&sql, [hash], |_| Ok(()))
        .optional()?
        .is_some())
}

/// New text for a row of these bytes. Empty text deletes it.
pub fn set_text(conn: &Connection, table: Table, hash: &str, row: &Row, text: &str) -> Result<()> {
    // A stage that ran again since the row was read replaced the rows, and
    // SQLite gives a new row the id of a deleted one. The text tells them apart.
    let changed = conn.execute(
        &format!(
            "UPDATE {} SET text = ?4, edited = 1 WHERE id = ?1 AND hash = ?2 AND text = ?3",
            table.name()
        ),
        params![row.id, hash, row.text, text.trim()],
    )?;
    if changed == 0 {
        bail!("the row changed while it was edited");
    }
    fts::refresh_hash(conn, hash)
}

pub fn delete(conn: &Connection, table: Table, hash: &str, row: &Row) -> Result<()> {
    set_text(conn, table, hash, row, "")
}

/// A new row at `time_ms`, and its id. A segment the user adds has no length.
pub fn add(
    conn: &Connection,
    table: Table,
    hash: &str,
    time_ms: Option<i64>,
    text: &str,
) -> Result<i64> {
    let text = text.trim();
    if text.is_empty() {
        bail!("a new row needs text");
    }
    match table {
        Table::Segments => conn.execute(
            "INSERT INTO segments (hash, start_ms, end_ms, text, edited)
             VALUES (?1, ?2, ?2, ?3, 1)",
            params![hash, time_ms.unwrap_or(0), text],
        )?,
        Table::Ocr => conn.execute(
            "INSERT INTO ocr (hash, time_ms, text, edited) VALUES (?1, ?2, ?3, 1)",
            params![hash, time_ms, text],
        )?,
    };
    let id = conn.last_insert_rowid();
    fts::refresh_hash(conn, hash)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ocr::{self, Line};
    use crate::db::{Db, segments};
    use crate::speech::Segment;

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

    fn segment(start_ms: i64, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms: start_ms + 1000,
            text: text.into(),
            confidence: None,
            no_speech_prob: None,
        }
    }

    fn texts(db: &Db, table: Table) -> Vec<String> {
        rows(db.conn(), table, "h")
            .unwrap()
            .into_iter()
            .map(|row| row.text)
            .collect()
    }

    fn indexed(db: &Db, column: &str) -> String {
        db.conn()
            .query_row(&format!("SELECT {column} FROM fts"), [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_new_run_does_not_replace_edited_rows() {
        let db = seeded();
        segments::replace(db.conn(), "h", &[segment(0, "helo"), segment(2000, "wrld")]).unwrap();
        let first = rows(db.conn(), Table::Segments, "h").unwrap();
        set_text(db.conn(), Table::Segments, "h", &first[0], "hello").unwrap();
        delete(db.conn(), Table::Segments, "h", &first[1]).unwrap();

        segments::replace(db.conn(), "h", &[segment(0, "a new run")]).unwrap();

        assert_eq!(texts(&db, Table::Segments), ["hello"]);
        assert_eq!(indexed(&db, "speech"), "hello");
        assert!(any(db.conn(), Table::Segments, "h").unwrap());
    }

    #[test]
    fn a_new_run_replaces_rows_nobody_edited() {
        let db = seeded();
        ocr::replace(db.conn(), "h", &[Line::new(0, "old")]).unwrap();
        ocr::replace(db.conn(), "h", &[Line::new(0, "new")]).unwrap();
        assert_eq!(texts(&db, Table::Ocr), ["new"]);
        assert!(!any(db.conn(), Table::Ocr, "h").unwrap());
    }

    #[test]
    fn an_added_row_is_an_edit() {
        let db = seeded();
        add(db.conn(), Table::Ocr, "h", Some(1500), "typed by hand").unwrap();
        ocr::replace(db.conn(), "h", &[Line::new(0, "read")]).unwrap();
        assert_eq!(texts(&db, Table::Ocr), ["typed by hand"]);
        assert_eq!(indexed(&db, "ocr"), "typed by hand");
    }

    #[test]
    fn an_edited_watermark_line_is_indexed() {
        let db = seeded();
        ocr::replace(
            db.conn(),
            "h",
            &[Line::new(0, "TikTok"), Line::new(0, "the caption")],
        )
        .unwrap();
        fts::refresh_hash(db.conn(), "h").unwrap();
        assert_eq!(indexed(&db, "ocr"), "the caption");

        let watermark = &rows(db.conn(), Table::Ocr, "h").unwrap()[0];
        set_text(db.conn(), Table::Ocr, "h", watermark, "TikTok").unwrap();

        assert_eq!(indexed(&db, "ocr"), "TikTok the caption");
    }

    #[test]
    fn an_edit_of_a_replaced_row_fails() {
        let db = seeded();
        ocr::replace(db.conn(), "h", &[Line::new(0, "old")]).unwrap();
        let old = rows(db.conn(), Table::Ocr, "h").unwrap().remove(0);
        ocr::replace(db.conn(), "h", &[Line::new(0, "new")]).unwrap();
        assert!(set_text(db.conn(), Table::Ocr, "h", &old, "mine").is_err());
        assert_eq!(texts(&db, Table::Ocr), ["new"]);
    }
}
