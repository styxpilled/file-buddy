# file-buddy

A fast search over a large collection of short clips. You type words you
remember, you get a grid of matching media, you copy the file and paste it
somewhere else.

It indexes video, GIFs and still images. Results rank over four signals, in this
order of trust: tags, filename, speech from Whisper, and text on screen from
OCR.

Windows only. Personal tool.

## State

v1 is complete: the index, the four signals, the watcher, tags, rename,
Review, Folders and Settings.

- `docs/source-layout.md` - where code goes in `src/`
- `scripts/drive.ps1` - type into the running window and take a picture of it.
  See `docs/agents/checking-the-window.md`
- `.scratch/file-buddy-v1/spec.md` - the v1 specification
- `.scratch/file-buddy-v1/issues/` - the work, numbered in the order it was done
- `.scratch/file-buddy-v1/research.md` - what the dependencies can and cannot do

## What is not in the repository

These are gitignored and must be present to build or run:

| Path | What |
| --- | --- |
| `ffmpeg/` | A gyan.dev full build of FFmpeg 8.x, shared, with `--enable-whisper`. `build.rs` copies its DLLs next to the executable |
| `models/` | `ggml-large-v3-turbo.bin` for transcription |
| `test_videos/` | Labelled clips to try the application on |

`.cargo/config.toml` points the FFmpeg build at `ffmpeg/`, with absolute paths.
Change them if the repository moves.

## Build

    cargo build
    cargo run

The first run writes `file-buddy.toml` next to the executable. Put your clip
folders in `roots` and run it again.

`fb` drives the index without a window. It reads the same `file-buddy.toml` and
writes the database and the thumbnails next to the executable.
`FILE_BUDDY_DATA` moves both.

    cargo run --bin fb -- scan            walk the roots and run every stage
    cargo run --bin fb -- search <words>  print the ranked results
    cargo run --bin fb -- reindex         rebuild the search index
    cargo run --bin fb -- status          counts per stage
