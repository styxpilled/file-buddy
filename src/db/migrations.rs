//! Schema migrations. Everything except `tags` and `file_tags` can be rebuilt
//! from the files on disk, so a migration may drop and recreate any other
//! table. A migration must never drop the two tag tables.

use rusqlite_migration::{M, Migrations};

pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(INITIAL),
        M::up(SKIP_WHISPER_WITHOUT_AUDIO),
        M::up(RETRY_OCR_WITHOUT_A_WORKER),
        M::up(FLATTEN_THE_OCR_INDEX),
        M::up(REREAD_TEXT_WITHOUT_THE_WATERMARK),
        M::up(RESTORE_EVERY_OCR_LINE),
    ])
}

/// The probe queued whisper for every video before issue 07. A file with no
/// audio track has nothing to transcribe.
const SKIP_WHISPER_WITHOUT_AUDIO: &str = r#"
UPDATE jobs SET state = 'skipped', error = 'no audio track'
WHERE stage = 'whisper' AND state = 'pending'
  AND hash IN (SELECT hash FROM files WHERE state = 'ready' AND audio_codec IS NULL);
"#;

/// Issue 10 gave `ocr` a worker. Until it did, every OCR job failed twice with
/// "has no worker yet" and marked its file failed, which put it in Review.
const RETRY_OCR_WITHOUT_A_WORKER: &str = r#"
UPDATE files SET state = 'ready', error = NULL
WHERE state = 'failed' AND error LIKE '%has no worker yet%';

UPDATE jobs SET state = 'pending', attempts = 0, error = NULL
WHERE stage = 'ocr' AND error LIKE '%has no worker yet%';
"#;

/// The first OCR index held one line per line of text, and the snippet of a
/// result carried the line breaks into the tile that shows it on one row.
const FLATTEN_THE_OCR_INDEX: &str = r#"
UPDATE fts SET ocr = replace(ocr, char(10), ' ')
WHERE ocr LIKE '%' || char(10) || '%';
"#;

/// The TikTok watermark was first dropped as a file was read, which left it in
/// every file already read. The rule has since moved to where the index is
/// built, and this migration is kept only because it ran.
const REREAD_TEXT_WITHOUT_THE_WATERMARK: &str = r#"
UPDATE jobs SET state = 'pending', attempts = 0, error = NULL
WHERE stage = 'ocr' AND state = 'done';
"#;

/// The `ocr` table now keeps every line the recognizer read, because the noise
/// rule moved to the index. A file read under the old rule is missing the lines
/// that rule dropped, so it is read once more.
const RESTORE_EVERY_OCR_LINE: &str = r#"
UPDATE jobs SET state = 'pending', attempts = 0, error = NULL
WHERE stage = 'ocr' AND state = 'done';
"#;

const INITIAL: &str = r#"
CREATE TABLE files (
    id           INTEGER PRIMARY KEY,
    path         TEXT NOT NULL UNIQUE,
    hash         TEXT NOT NULL,
    filename     TEXT NOT NULL,
    kind         TEXT NOT NULL,
    size         INTEGER NOT NULL,
    mtime        INTEGER NOT NULL,
    duration     REAL,
    width        INTEGER,
    height       INTEGER,
    video_codec  TEXT,
    audio_codec  TEXT,
    state        TEXT NOT NULL DEFAULT 'pending',
    error        TEXT,
    seen_at      INTEGER NOT NULL
);

-- Not unique. Two paths with the same bytes are a duplicate group, and every
-- stage result is keyed by the hash, so the work happens once.
CREATE INDEX files_hash ON files (hash);
CREATE INDEX files_state ON files (state);

CREATE TABLE segments (
    id         INTEGER PRIMARY KEY,
    hash       TEXT NOT NULL,
    start_ms   INTEGER NOT NULL,
    end_ms     INTEGER NOT NULL,
    text       TEXT NOT NULL,
    confidence REAL
);

CREATE INDEX segments_hash ON segments (hash);

-- One row per file, holding the deduplicated lines that OCR read from it.
CREATE TABLE ocr (
    hash TEXT PRIMARY KEY,
    text TEXT NOT NULL
);

CREATE TABLE tags (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE file_tags (
    hash   TEXT NOT NULL,
    tag_id INTEGER NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    PRIMARY KEY (hash, tag_id)
);

CREATE INDEX file_tags_tag ON file_tags (tag_id);

-- The rowid is files.id. Four columns, one query, weights from the config.
CREATE VIRTUAL TABLE fts USING fts5 (
    tags, filename, speech, ocr,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE jobs (
    hash       TEXT NOT NULL,
    stage      TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'pending',
    attempts   INTEGER NOT NULL DEFAULT 0,
    error      TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (hash, stage)
);

CREATE INDEX jobs_state ON jobs (state, stage);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_migrations_are_valid() {
        migrations().validate().unwrap();
    }
}
