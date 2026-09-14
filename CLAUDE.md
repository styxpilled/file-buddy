This is file-buddy - a fast app for finding your clips. You type words you remember, you get a grid of matching media, you copy the file. It indexes video, GIFs and still images, ranked over four signals: tags, filename, speech and on-screen text. It's only for my use, so no need to worry about making it work in non-Windows environments.

The v1 spec is `.scratch/file-buddy-v1/spec.md`. Read it before any design work.

Stack: Rust, `gpui-kit` 0.6.1 (gpui-pre, not crates.io gpui) for UI, `rusqlite` with bundled SQLite and FTS5 for data, the FFmpeg CLI for probing, thumbnails and Whisper transcription, and `Windows.Media.Ocr` for on-screen text. `gpui-video` is vendored as project code and diverges from upstream. All versions are pinned exactly: gpui-pre is an unreviewed weekly snapshot of Zed's main branch.

# Approach

- Use ASD-STE100 Simplified Technical English.
- Thorough in reasoning, concise in output.
- No sycophantic openers or closing fluff.
- Minimal, concise comments. Good code is self-documenting.
- No over-engineering.
- No em-dashes.

## Issue tracker

Issues and specs for this repo live as markdown files in `.scratch/`. See `/docs/agents/issue-tracker.md`.
