# Basics

This is file-buddy - a fast app for finding local clips. It indexes video, GIFs and still images, ranked over four signals: tags, filename, speech and on-screen text. It's only for my use, so no need to worry about making it work in non-Windows environments.

## Approach (IMPORTANT!)

- Use ASD-STE100 Simplified Technical English.
- Thorough in reasoning, concise in output.
- No sycophantic openers or closing fluff.
- Minimal, concise comments. Good code is self-documenting.
- Remove all mannered prose.
- No over-engineering.
- No em-dashes.

## Additional info

The v1 spec is `.scratch/file-buddy-v1/spec.md`. Read it before any design work.

Stack: Rust, `gpui-kit`, `rusqlite` with bundled SQLite and FTS5, FFmpeg for probing, thumbnails and Whisper transcription, and `Windows.Media.Ocr` for on-screen text. `gpui-video` is vendored as project code. All versions are pinned exactly: gpui-pre is an unreviewed weekly snapshot of Zed's main branch.

Any changes you haven't made are probably deliberate. If your build fails because I'm running the app, don't try to work around that. Simply report the work done and ask if I want you to test any behavior you've just implemented.

## Running the app

My release build is often open while you work. Ask me before you drive the window, unless I said yes a short time ago in this session. Drive and stop only the debug build. See `docs/agents/checking-the-window.md`. During the work, check with `scripts/sim.ps1` and a `--features sim` dev build. It needs no question. If a 100% concrete test is needed, use `drive.ps1`.

## Issue tracker

Issues and specs for this repo live as markdown files in `.scratch/`. See `/docs/agents/issue-tracker.md`.
