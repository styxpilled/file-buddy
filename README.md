# file-buddy

A fast search over a large collection of short clips. You type words you
remember, you get a grid of matching media, you copy the file and paste it
somewhere else.

It indexes video, GIFs and still images. Results rank over four signals, in this
order of trust: tags, filename, speech from Whisper, and text on screen from
OCR.

Windows only. Personal tool.

## State

Early. What exists today is a video player prototype and a finished design.

- `.scratch/file-buddy-v1/spec.md` - the v1 specification
- `.scratch/file-buddy-v1/issues/` - the work, numbered in the order to do it
- `.scratch/file-buddy-v1/research.md` - what the dependencies can and cannot do
- `spike/ocr/` - the OCR spike from issue 01, kept until issue 10 is done

Issue 01 is done. Issue 02 is next.

## What is not in the repository

These are gitignored and must be present to build or run:

| Path | What |
| --- | --- |
| `ffmpeg/` | A gyan.dev full build of FFmpeg 8.x, shared, with `--enable-whisper`. `build.rs` copies its DLLs next to the executable |
| `gpui-video/` | The video component. Issue 02 vendors it into the repository |
| `models/` | `ggml-large-v3-turbo.bin` for transcription. Needed from issue 07 |
| `test_videos/` | Labelled clips used by the OCR spike |

`.cargo/config.toml` points the FFmpeg build at `ffmpeg/`, with absolute paths.
Change them if the repository moves.

## Build

    cargo build
    cargo run -- "path/to/clip.mp4"

The OCR spike is a separate crate:

    cd spike/ocr
    cargo run --release -- <directory of extracted frames>
