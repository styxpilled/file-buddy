# 03 - Schema, migrations and the scan pipeline

Status: ready-for-agent

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
