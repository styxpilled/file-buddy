//! The files that need a person: duplicates, long files, failures and poor
//! names. Each comes with a note that says why it is here.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::db::files::{self, FileRow, PRESENT, read_row};
use crate::index::jobs::LONG_FILE_SECONDS;
use crate::media::filename;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Duplicates,
    Long,
    Failed,
    PoorNames,
}

impl Group {
    pub const ALL: [Group; 4] = [
        Group::Duplicates,
        Group::Long,
        Group::Failed,
        Group::PoorNames,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Duplicates => "Duplicates",
            Group::Long => "Long",
            Group::Failed => "Failed",
            Group::PoorNames => "Poor names",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub file: FileRow,
    pub note: String,
}

pub fn items(conn: &Connection, group: Group) -> Result<Vec<Item>> {
    match group {
        Group::Duplicates => duplicates(conn),
        Group::Long => long(conn),
        Group::Failed => failed(conn),
        Group::PoorNames => poor_names(conn),
    }
}

/// Every path that shares its bytes with another, one group after the other.
fn duplicates(conn: &Connection) -> Result<Vec<Item>> {
    let mut statement = conn.prepare(&format!(
        "{PRESENT} AND hash IN (
             SELECT hash FROM files WHERE missing_at IS NULL
             GROUP BY hash HAVING count(*) > 1)
         ORDER BY hash, path"
    ))?;
    let rows: Vec<FileRow> = statement
        .query_map([], read_row)?
        .collect::<Result<_, _>>()?;

    let mut items = Vec::with_capacity(rows.len());
    for group in rows.chunk_by(|a, b| a.hash == b.hash) {
        for (at, file) in group.iter().enumerate() {
            items.push(Item {
                note: format!(
                    "copy {} of {}  ·  {}",
                    at + 1,
                    group.len(),
                    folder(&file.path)
                ),
                file: file.clone(),
            });
        }
    }
    Ok(items)
}

/// The longest first, because those are the likeliest to be deleted.
fn long(conn: &Connection) -> Result<Vec<Item>> {
    let mut statement = conn.prepare(&format!(
        "{PRESENT} AND duration > ?1 ORDER BY duration DESC"
    ))?;
    let rows = statement.query_map([LONG_FILE_SECONDS], read_row)?;
    rows.map(|row| {
        let file = row?;
        Ok(Item {
            note: clock(file.duration.unwrap_or_default()),
            file,
        })
    })
    .collect()
}

fn failed(conn: &Connection) -> Result<Vec<Item>> {
    let mut statement = conn.prepare(
        "SELECT id, path, hash, filename, kind, size, duration, state, mtime, error
         FROM files WHERE missing_at IS NULL AND state = 'failed' ORDER BY path",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Item {
            file: read_row(row)?,
            note: row
                .get::<_, Option<String>>(9)?
                .unwrap_or_else(|| "no error was recorded".into()),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Ordered by path, so the files of one folder come one after another. The rule
/// is in Rust, because SQL cannot say it simply.
fn poor_names(conn: &Connection) -> Result<Vec<Item>> {
    Ok(files::all(conn)?
        .into_iter()
        .filter(|file| {
            file.path
                .file_stem()
                .is_some_and(|stem| filename::is_poor(&stem.to_string_lossy()))
        })
        .map(|file| Item {
            note: folder(&file.path),
            file,
        })
        .collect())
}

/// The last folder of the path. A tile has room for little, and the root the
/// folders share says nothing. The side pane shows the whole path.
fn folder(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `12:04`, or `1:02:30` past an hour.
fn clock(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::media::Kind;

    fn add(db: &Db, path: &str, hash: &str) -> i64 {
        files::upsert(
            db.conn(),
            &files::Scanned {
                path: Path::new(path),
                hash,
                kind: Kind::Video,
                size: 1,
                mtime: 0,
            },
        )
        .unwrap()
        .0
    }

    fn paths(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| item.file.path.display().to_string())
            .collect()
    }

    #[test]
    fn duplicates_come_in_groups() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "d/b.mp4", "one");
        add(&db, "d/a.mp4", "one");
        add(&db, "d/alone.mp4", "two");

        let found = items(db.conn(), Group::Duplicates).unwrap();

        assert_eq!(paths(&found), ["d/a.mp4", "d/b.mp4"]);
        assert_eq!(found[1].note, "copy 2 of 2  ·  d");
    }

    #[test]
    fn long_files_are_longest_first() {
        let db = Db::open_in_memory().unwrap();
        for (path, hash, duration) in [
            ("short.mp4", "a", 299.0),
            ("long.mp4", "b", 301.0),
            ("longer.mp4", "c", 3750.0),
        ] {
            let id = add(&db, path, hash);
            db.conn()
                .execute(
                    "UPDATE files SET duration = ?2 WHERE id = ?1",
                    (id, duration),
                )
                .unwrap();
        }

        let found = items(db.conn(), Group::Long).unwrap();

        assert_eq!(paths(&found), ["longer.mp4", "long.mp4"]);
        assert_eq!(found[0].note, "1:02:30");
        assert_eq!(found[1].note, "5:01");
    }

    #[test]
    fn failures_carry_their_error() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "fine.mp4", "a");
        add(&db, "broken.mp4", "b");
        files::mark_failed(db.conn(), "b", "moov atom not found").unwrap();

        let found = items(db.conn(), Group::Failed).unwrap();

        assert_eq!(paths(&found), ["broken.mp4"]);
        assert_eq!(found[0].note, "moov atom not found");
    }

    #[test]
    fn poor_names_follow_the_rule() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "c/my honest reaction.mp4", "a");
        add(&db, "c/IMG_2041.mp4", "b");

        let found = items(db.conn(), Group::PoorNames).unwrap();

        assert_eq!(paths(&found), ["c/IMG_2041.mp4"]);
    }
}
