# 03 - Schema, migrations and the scan pipeline

Status: done

Everything the application shows comes from this. No UI in this issue.

## Schema

- `files` - content hash as the key, path, media kind, size, mtime, duration,
  width, height, codecs, indexed state, error text
- `segments` - speech segments with start, end, text and confidence, for later
  use. Not ranked in v1
- `tags` and `file_tags` - the only tables that cannot be rebuilt. Migrations
  must never drop them
- `fts` - FTS5 with the columns `tags`, `filename`, `speech`, `ocr`
- `jobs` - file hash, stage, state, attempt count, error

`rusqlite` 0.40 with `bundled`, and `rusqlite_migration`. FTS5 and the trigram
tokenizer are compiled in by the bundled build with no extra flag.

## Identity

blake3 over the size, the first 1 MB and the last 1 MB. A rename updates the
filename column only. It never runs an expensive stage again.

## Config

TOML next to the executable, created on first run:

    roots = ["D:/clips"]
    ignore = ["**/thumbs/**", "**/.git/**"]
    weights = { tags = 20.0, filename = 10.0, speech = 3.0, ocr = 1.0 }

`FILE_BUDDY_DATA` moves the database and the thumbnail cache.

## Stages

`probe` with ffprobe, then `thumbnail` with the FFmpeg `thumbnail` filter, which
avoids a black opening frame. One code path for every media kind, including
still images. Thumbnails go to a cache directory keyed by content hash.

Probe and thumbnail run 8 at a time. Two failed attempts then stop. Files over 5
minutes skip the expensive stages but stay findable by name. Files that FFmpeg
cannot open record the error and are never retried.

## Filename tokenization

Split on `_ - . ( )` and camelCase, lowercase, index the split form. Do not
remove noise tokens such as `1080p`. Stripping is a guess, and a wrong guess
makes a file impossible to find with no error.

## Done when

A scan of a real folder fills the database, writes thumbnails, and an FTS query
from a test returns the right files by filename.

## Comments

### 2026-09-14, agent

Done. `cargo test` passes: 30 unit tests and 6 integration tests. `cargo clippy
--all-targets` is clean for project code.

The crate now has a library target. `src/lib.rs` holds the data layer and knows
nothing about the user interface. `src/main.rs` is untouched and is still the
issue 02 video prototype.

| Directory | What it holds |
| --- | --- |
| `config.rs`, `paths.rs` | The TOML file, the weights, the data directory |
| `media/` | The three kinds, the hash, the filename split |
| `db/` | Open, pragmas, migrations, and every query |
| `ffmpeg/` | Tool lookup, the shared command, ffprobe, the thumbnail filter |
| `index/` | The queue, the walk, the worker pool, and one file per stage |
| `search/` | Query construction and the ranked query |

`docs/source-layout.md` says where the work of issues 04 to 11 goes. Issues 07
and 10 are additions to `index/stages/`, not changes to the worker pool.

`src/bin/fb.rs` is a headless front end with `scan`, `search` and `status`, so
the pipeline can be run and checked before issue 04 exists.

### Decisions the issue left open

**The hash is not unique in `files`.** The path is. Two paths with the same
bytes are a duplicate group, which the Review screen needs in issue 09. Every
stage result is keyed by the hash instead, so the work still happens once.
`files` also has an `id`, which is the rowid of the `fts` row.

**A new path with a known hash copies the probe result** from the file it
duplicates. This is what makes a rename free: the row is ready the moment the
scan sees it, and no stage runs.

**OCR text needs a home**, and the issue does not name one. It is an `ocr`
table with one row per hash, next to `segments`. Both are rebuildable, so `fts`
stays derived from tables that own their text and nothing is a source of truth
twice.

**`ffprobe` has no `-nostdin` option** and exits with an error if it gets one.
Both calls close standard input instead, which is what the flag is for.
`ffmpeg` still gets `-nostdin` as well.

**Claims use an immediate transaction.** A deferred transaction that reads and
then writes fails at once with `SQLITE_BUSY_SNAPSHOT` under WAL, and
`busy_timeout` does not retry that. Eight workers hit it immediately.

`whisper` and `ocr` jobs are queued but have no worker, so they sit pending
until issues 07 and 10. A file longer than five minutes gets those two stages
recorded as `skipped` with the reason, and keeps its probe, its thumbnail and
its filename in the index.

### Measured

A scan of `test_videos/`, 21 clips, from an empty database: 21 probes and 21
thumbnails in 1.1 seconds, 440 kB of thumbnails. A second scan does no work.
`fb search "tony sopr"` returns the Sopranos clip and nothing else.
