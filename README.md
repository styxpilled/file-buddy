# file-buddy

A fast search over a large collection of short clips. You type words you
remember, you get a grid of matching media, you copy the file and paste it
somewhere else.

It indexes video, GIFs and still images. Results rank over four signals, in this
order of trust: tags, filename, speech from Whisper, and text on screen from
OCR.

Windows only. Personal tool.

## State

Early. The index and the search window work. Speech and screen text are not
indexed yet, so results rank on tags and the filename.

- `docs/source-layout.md` - where code goes in `src/`, and where the work of
  the remaining issues goes
- `scripts/drive.ps1` - type into the running window and take a picture of it.
  See `docs/agents/checking-the-window.md`
- `.scratch/file-buddy-v1/spec.md` - the v1 specification
- `.scratch/file-buddy-v1/issues/` - the work, numbered in the order to do it
- `.scratch/file-buddy-v1/research.md` - what the dependencies can and cannot do
- `spike/ocr/` - the OCR spike from issue 01, kept until issue 10 is done

Issues 01 to 04 are done. Issue 05, the file actions, is next.

## What is not in the repository

These are gitignored and must be present to build or run:

| Path | What |
| --- | --- |
| `ffmpeg/` | A gyan.dev full build of FFmpeg 8.x, shared, with `--enable-whisper`. `build.rs` copies its DLLs next to the executable |
| `gpui-video/` | The video component, vendored in issue 02 |
| `models/` | `ggml-large-v3-turbo.bin` for transcription. Needed from issue 07 |
| `test_videos/` | Labelled clips used by the OCR spike |

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
    cargo run --bin fb -- status          counts per stage

The OCR spike is a separate crate:

    cd spike/ocr
    cargo run --release -- <directory of extracted frames>
