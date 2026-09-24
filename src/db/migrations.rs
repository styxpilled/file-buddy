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
        M::up(MARK_A_GONE_FILE_MISSING),
        M::up(RECORD_NO_SPEECH_PROB),
        M::up(ADD_THE_CLIP_STAGE),
        M::up(OCR_LINES_WITH_TIMES),
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

/// Issue 11. A file that is gone keeps its row, so its tags and its finished
/// stages come back with it. Everything that shows files hides the missing ones.
const MARK_A_GONE_FILE_MISSING: &str = r#"
ALTER TABLE files ADD COLUMN missing_at INTEGER;
CREATE INDEX files_missing ON files (missing_at);
"#;

const RECORD_NO_SPEECH_PROB: &str = r#"
ALTER TABLE segments ADD COLUMN no_speech_prob REAL;
"#;

/// Issue 20. One CLIP vector per sampled frame. Every file already probed gets a
/// `clip` job, skipped where its OCR job was skipped, since both stages sample
/// the same frames under the same limit.
const ADD_THE_CLIP_STAGE: &str = r#"
CREATE TABLE vectors (
    id      INTEGER PRIMARY KEY,
    hash    TEXT NOT NULL,
    time_ms INTEGER NOT NULL,
    vector  BLOB NOT NULL
);

CREATE INDEX vectors_hash ON vectors (hash);

INSERT OR IGNORE INTO jobs (hash, stage, state, error, updated_at)
SELECT hash, 'clip',
       CASE state WHEN 'skipped' THEN 'skipped' ELSE 'pending' END,
       CASE state WHEN 'skipped' THEN error END,
       unixepoch()
FROM jobs WHERE stage = 'ocr';
"#;

/// Issue 24. One row per line, with the time of the first frame that shows it.
/// The lines already read move over with no time, so search keeps them until
/// the file is read again, which every file is.
const OCR_LINES_WITH_TIMES: &str = r#"
CREATE TABLE ocr_lines (
    id      INTEGER PRIMARY KEY,
    hash    TEXT NOT NULL,
    time_ms INTEGER,
    text    TEXT NOT NULL
);

INSERT INTO ocr_lines (hash, time_ms, text)
WITH RECURSIVE split (hash, line, rest) AS (
    SELECT hash, '', text || char(10) FROM ocr
    UNION ALL
    SELECT hash,
           substr(rest, 1, instr(rest, char(10)) - 1),
           substr(rest, instr(rest, char(10)) + 1)
    FROM split WHERE rest <> ''
)
SELECT hash, NULL, line FROM split WHERE line <> '';

DROP TABLE ocr;
ALTER TABLE ocr_lines RENAME TO ocr;
CREATE INDEX ocr_hash ON ocr (hash);

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

    #[test]
    fn the_ocr_text_moves_to_one_row_per_line() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        // Version 9 is the last with one row of text per file.
        migrations().to_version(&mut conn, 9).unwrap();
        conn.execute(
            "INSERT INTO ocr (hash, text) VALUES ('h', 'first' || char(10) || 'second'), ('e', '')",
            [],
        )
        .unwrap();
        migrations().to_latest(&mut conn).unwrap();

        let lines: Vec<(String, Option<i64>, String)> = conn
            .prepare("SELECT hash, time_ms, text FROM ocr ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            lines,
            vec![
                ("h".into(), None, "first".into()),
                ("h".into(), None, "second".into())
            ]
        );
    }
}
