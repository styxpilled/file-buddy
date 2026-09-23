//! Rows in `files`. A file is identified on disk by its path and by content
//! through its hash. Every stage result is keyed by the hash.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::db::{fts, now};
use crate::ffmpeg::probe::Probe;
use crate::media::Kind;
use crate::media::filename;

/// The state of the whole file, as a summary of its stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Pending,
    Ready,
    /// FFmpeg cannot open it. A rescan does not retry it.
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Ready => "ready",
            State::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileRow {
    pub id: i64,
    pub path: PathBuf,
    pub hash: String,
    pub filename: String,
    pub kind: Kind,
    pub size: u64,
    pub duration: Option<f64>,
    pub state: String,
}

impl FileRow {
    /// The last part of a path, which is what the `filename` column holds.
    pub fn name_of(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// What an upsert changed, which decides whether the scan queues work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upsert {
    /// The path is new to the database.
    Added,
    /// The path is known and the bytes changed.
    Changed,
    /// The path is known and the bytes did not change.
    Unchanged,
}

pub struct Scanned<'a> {
    pub path: &'a Path,
    pub hash: &'a str,
    pub kind: Kind,
    pub size: u64,
    pub mtime: i64,
}

/// Insert the file, or update the row that holds this path. A rename is a new
/// path with a known hash, so it costs one insert and no stage work.
pub fn upsert(conn: &Connection, file: &Scanned<'_>) -> Result<(i64, Upsert)> {
    let filename = file
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = file.path.to_string_lossy();

    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, hash FROM files WHERE path = ?1",
            [path.as_ref()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    match existing {
        Some((id, hash)) if hash == file.hash => {
            conn.execute(
                "UPDATE files SET filename = ?2, size = ?3, mtime = ?4, seen_at = ?5,
                        missing_at = NULL
                 WHERE id = ?1",
                params![id, filename, file.size as i64, file.mtime, now()],
            )?;
            Ok((id, Upsert::Unchanged))
        }
        Some((id, _)) => {
            conn.execute(
                "UPDATE files SET hash = ?2, filename = ?3, kind = ?4, size = ?5, mtime = ?6,
                        duration = NULL, width = NULL, height = NULL, video_codec = NULL,
                        audio_codec = NULL, state = 'pending', error = NULL, seen_at = ?7,
                        missing_at = NULL
                 WHERE id = ?1",
                params![
                    id,
                    file.hash,
                    filename,
                    file.kind.as_str(),
                    file.size as i64,
                    file.mtime,
                    now()
                ],
            )?;
            Ok((id, Upsert::Changed))
        }
        None => {
            conn.execute(
                "INSERT INTO files (path, hash, filename, kind, size, mtime, seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    path,
                    file.hash,
                    filename,
                    file.kind.as_str(),
                    file.size as i64,
                    file.mtime,
                    now()
                ],
            )?;
            let id = conn.last_insert_rowid();
            inherit_from_duplicate(conn, id, file.hash)?;
            Ok((id, Upsert::Added))
        }
    }
}

/// Take the probe result from another file that holds the same bytes. This is
/// what makes a rename cheap: the new path is ready the moment the scan sees it.
fn inherit_from_duplicate(conn: &Connection, id: i64, hash: &str) -> Result<()> {
    conn.execute(
        "UPDATE files SET kind = source.kind, duration = source.duration, width = source.width,
                height = source.height, video_codec = source.video_codec,
                audio_codec = source.audio_codec, state = source.state, error = source.error
         FROM (SELECT * FROM files WHERE hash = ?2 AND id <> ?1 AND missing_at IS NULL LIMIT 1)
              AS source
         WHERE files.id = ?1",
        params![id, hash],
    )?;
    Ok(())
}

/// Write what ffprobe found to every file that holds these bytes.
pub fn save_probe(conn: &Connection, hash: &str, probe: &Probe) -> Result<()> {
    conn.execute(
        "UPDATE files SET duration = ?2, width = ?3, height = ?4, video_codec = ?5,
                audio_codec = ?6, kind = ?7, state = 'ready', error = NULL
         WHERE hash = ?1",
        params![
            hash,
            probe.duration,
            probe.width,
            probe.height,
            probe.video_codec,
            probe.audio_codec,
            probe.kind.as_str(),
        ],
    )?;
    Ok(())
}

/// Record that FFmpeg cannot read these bytes. A rescan does not retry them.
pub fn mark_failed(conn: &Connection, hash: &str, error: &str) -> Result<()> {
    conn.execute(
        "UPDATE files SET state = 'failed', error = ?2 WHERE hash = ?1",
        params![hash, error],
    )?;
    Ok(())
}

pub fn by_hash(conn: &Connection, hash: &str) -> Result<Vec<FileRow>> {
    let mut statement = conn.prepare(&format!("{PRESENT} AND hash = ?1"))?;
    let rows = statement.query_map([hash], read_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn by_path(conn: &Connection, path: &Path) -> Result<Option<FileRow>> {
    let mut statement = conn.prepare(&format!("{SELECT} WHERE path = ?1"))?;
    Ok(statement
        .query_row([path.to_string_lossy()], read_row)
        .optional()?)
}

pub fn all(conn: &Connection) -> Result<Vec<FileRow>> {
    let mut statement = conn.prepare(&format!("{PRESENT} ORDER BY path"))?;
    let rows = statement.query_map([], read_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Point a row at its new path. The hash does not change, so every stage
/// result and every tag stays. Only the filename column of the index changes.
pub fn set_path(conn: &Connection, id: i64, path: &Path) -> Result<()> {
    conn.execute(
        "UPDATE files SET path = ?2, filename = ?3 WHERE id = ?1",
        params![id, path.to_string_lossy(), FileRow::name_of(path)],
    )?;
    fts::refresh(conn, id)
}

/// The files at this path, or under it when a whole folder is gone.
pub fn rows_under(conn: &Connection, path: &Path) -> Result<Vec<FileRow>> {
    let prefix = format!("{}{}", path.to_string_lossy(), std::path::MAIN_SEPARATOR);
    let mut statement = conn.prepare(&format!(
        "{PRESENT} AND (path = ?1 OR substr(path, 1, length(?2)) = ?2)"
    ))?;
    let rows = statement.query_map(params![path.to_string_lossy(), prefix], read_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// A path that is gone. The row stays, so the tags and the finished stages of
/// these bytes come back with the file. A row whose bytes a present path still
/// holds carries nothing of its own, so that one goes.
pub fn mark_gone(conn: &Connection, id: i64) -> Result<()> {
    let redundant = conn.execute(
        "DELETE FROM files WHERE id = ?1 AND EXISTS (
             SELECT 1 FROM files AS other
             WHERE other.hash = files.hash AND other.id <> files.id
               AND other.missing_at IS NULL)",
        [id],
    )?;
    if redundant == 0 {
        conn.execute(
            "UPDATE files SET missing_at = ?2 WHERE id = ?1 AND missing_at IS NULL",
            params![id, now()],
        )?;
    }
    // Deleted, so the index row goes with it. Missing, so it is out of the way
    // until the file comes back.
    fts::refresh(conn, id)
}

/// Mark every path that the walk did not see. Returns how many are now missing.
pub fn mark_missing(conn: &mut Connection, seen_ids: &[i64]) -> Result<usize> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "CREATE TEMP TABLE IF NOT EXISTS seen (id INTEGER PRIMARY KEY)",
        [],
    )?;
    transaction.execute("DELETE FROM seen", [])?;
    {
        let mut insert = transaction.prepare("INSERT OR IGNORE INTO seen (id) VALUES (?1)")?;
        for id in seen_ids {
            insert.execute([id])?;
        }
    }
    let unseen: Vec<i64> = {
        let mut statement = transaction.prepare(
            "SELECT id FROM files WHERE missing_at IS NULL AND id NOT IN (SELECT id FROM seen)",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for id in &unseen {
        mark_gone(&transaction, *id)?;
    }
    // Stage results of bytes that no file holds any more.
    for table in ["segments", "ocr", "jobs"] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE hash NOT IN (SELECT hash FROM files)"),
            [],
        )?;
    }
    transaction.commit()?;
    Ok(unseen.len())
}

/// The filename as the search index holds it.
pub fn indexed_filename(filename: &str) -> String {
    filename::index_form(filename)
}

pub(crate) const SELECT: &str =
    "SELECT id, path, hash, filename, kind, size, duration, state FROM files";

/// The same, of the files that are on disk. Every screen uses this one.
pub(crate) const PRESENT: &str =
    "SELECT id, path, hash, filename, kind, size, duration, state FROM files
     WHERE missing_at IS NULL";

pub(crate) fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRow> {
    let kind: String = row.get(4)?;
    Ok(FileRow {
        id: row.get(0)?,
        path: PathBuf::from(row.get::<_, String>(1)?),
        hash: row.get(2)?,
        filename: row.get(3)?,
        kind: kind.parse().unwrap_or(Kind::Still),
        size: row.get::<_, i64>(5)? as u64,
        duration: row.get(6)?,
        state: row.get(7)?,
    })
}
