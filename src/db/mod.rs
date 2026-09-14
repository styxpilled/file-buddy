//! The SQLite database: opening, pragmas and migrations.

pub mod files;
pub mod fts;
mod migrations;
pub mod tags;

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::Connection;

/// How long a writer waits for another writer before it gives up.
const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open the database, creating and migrating it if needed. Every worker
    /// opens its own connection. WAL lets readers run while one writer works.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        Self::prepare(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut conn = conn;
        migrations::migrations()
            .to_latest(&mut conn)
            .context("migrate the database")?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// Seconds since the Unix epoch, for the `seen_at` and `updated_at` columns.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opened_database_has_the_schema() {
        let db = Db::open_in_memory().unwrap();
        let tables: Vec<String> = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type IN ('table','view') ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for expected in [
            "files",
            "segments",
            "tags",
            "file_tags",
            "fts",
            "jobs",
            "ocr",
        ] {
            assert!(tables.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn fts5_is_available() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO fts (rowid, tags, filename, speech, ocr) VALUES (1,'a','b','c','d')",
                [],
            )
            .unwrap();
        let count: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM fts WHERE fts MATCH 'b'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}
