# file-buddy v1 spec

## Purpose

file-buddy finds a specific clip in a collection of a few thousand short, chaotic
videos. You type words you remember. You get a grid of matching media. You copy
the file. You paste it into another application.

Search is the product. Browsing is what the app shows when the query is empty.

## Scope

Media types, all first-class:

| Kind       | Examples              | Hover behaviour     | Stages that run             |
| ---------- | --------------------- | ------------------- | --------------------------- |
| `video`    | mp4, mkv, webm        | plays looping, mute | probe, thumbnail, whisper, ocr |
| `animated` | gif, animated webp    | plays looping, mute | probe, thumbnail, ocr       |
| `still`    | png, jpg, webp        | none                | probe, thumbnail, ocr       |

A result is a file. The application stores speech segment timestamps, but it does
not rank or show them in v1.

## Ranking

One FTS5 table with four columns. One query. Weights are configurable.

    bm25(fts, 20.0, 10.0, 3.0, 1.0)
            tags  filename  speech  ocr

The filename is the signal the user controls, so it outranks machine text. Tags
outrank the filename because the user applies them on purpose.

Search is per keystroke. Each query token becomes a prefix term (`tok*`). The
`tag:` operator limits a term to the tags column. Bare words still match tags
through the weighted column.

Filenames are split on `_ - . ( )` and on camelCase, then lowercased. The
application does not strip noise tokens such as `1080p`. Stripping is a guess,
and a wrong guess makes a file impossible to find with no error.

## Data

Identity is a content hash (blake3 of the size, the first 1 MB and the last
1 MB). A rename or a move is a metadata update. Duplicate files are found for
free.

The database and the thumbnail cache sit next to the executable. The environment
variable `FILE_BUDDY_DATA` moves both, to keep them out of `target/`.

Tags exist only in the database. They are the one kind of data that the
application cannot rebuild from the files. Migrations must never drop or
recreate the tags table.

## Indexing

A durable job queue in SQLite. One row per file per stage, so a failure in one
stage does not discard the result of another.

Stages: `probe` -> `thumbnail` -> `whisper` -> `ocr`

- Probe and thumbnail run 8 at a time. Whisper runs 1 or 2 at a time.
- After 2 failed attempts, the job records the error and stops.
- Files longer than 5 minutes get probe, thumbnail and filename indexing only.
  They appear in Review as candidates for the user to delete outside the app.
- Files that FFmpeg cannot open are recorded with their error. A rescan does not
  retry them.

Transcription uses the FFmpeg whisper filter with `ggml-large-v3-turbo.bin`, on
the GPU. Content is 98% English, but the model stays multilingual. The filter
gives start, end and text only, with no probabilities, so the application drops
text by rules: known hallucination phrases, sound tags such as `*music*`, and the
segments of a last window shorter than 2 seconds. `segments.confidence` stays
NULL. whisper-rs is the path to real probabilities if the rules are not enough.
See `issues/07-whisper-stage.md`.

OCR samples 10 frames spread over the duration of the file (`fps = 10/duration`,
limited to 4 fps), and removes duplicate lines before it indexes them. Scene
detection was measured against this and lost the text in 6 of 15 test clips. See
`issues/01-ocr-spike.md`.

Every FFmpeg call must pass `-nostdin`. FFmpeg reads standard input, which
corrupts any loop that feeds it file names.

## User interface

Keyboard first. The mouse is optional.

- The window holds focus, not the search box. `f` or a click focuses the box,
  escape or a click outside it gives focus back to the window. Only the focused
  box takes typed characters, which leaves the letters free for the window.
- Arrow keys move the selection in a virtual grid.
- Enter copies the file to the clipboard as `CF_HDROP` and as path text.
  `ctrl-e` reveals it in Explorer, `ctrl-o` opens it in the default
  application. The header confirms a copy for a couple of seconds.
- `m` mutes the side pane. `space` plays or pauses it, for that clip only.
- The wheel over the side pane, and `ctrl-up` and `ctrl-down`, set the sound
  level. Turning it up lifts the mute, because the press asks to hear the clip.
  The header shows the level beside the chips, and says `muted` instead of a
  number, which is the only thing that explains the silence. There is no
  control to drag: the level has one place to read it and two ways to set it.
- `ctrl-space` is the autoplay flag, off at the start, which decides whether a
  new selection starts playing in the side pane. It also plays or pauses the
  current clip. It works whether or not the search box holds focus, and it
  never touches a tile.
- Hover starts one live video in the tile. Only one plays at a time.
- A resizable side pane plays the selection and shows its transcript.
- Each result shows the matched text and a badge: tag, filename, speech or
  screen text.
- Chips filter by media kind. All are on by default.
- `t` opens a tag input with autocomplete over the tags that already exist.
  Tags are lowercase. `x` marks the selection and ctrl-click marks a tile, so
  one tag can reach many files at once. Inside the input, `alt-up` and
  `alt-down` pick a suggestion, because the plain arrows belong to the text.
  Enter applies.
- `r`, `ctrl-r` or `F2` renames the selection in the side pane. The input holds
  the stem only; the extension stays. Enter saves.
- Escape undoes one thing at a time, the most recent first: it closes the tag
  panel, or closes the rename input, or takes focus out of a Settings field, or
  goes back to Browse, or gives focus back from the search box, or clears the
  marks. A click outside the tag panel closes it
  too.

The window has three pages, with tabs at the start of the header: Browse,
Review and Settings. `ctrl-1` to `ctrl-3` switch between them.

The Review page collects every file that needs a person: duplicate groups,
files over 5 minutes, files that cannot be indexed, and files with no proper
name. It opens on the first group that holds a file, and `alt-1` to `alt-4`
pick a group. The group tabs take the place of the search box and the chips;
the grid and the side pane stay. Each tile says why the file
is there, and the side pane shows its folder and, for a failure, the error.

A name is poor when its stem holds `_`, `-`, a bracket, a character outside
ASCII or a number longer than 4 digits. The Poor names group is the bulk rename
walk. The clip plays, the user types a name, Enter moves to the next file.

The Settings page edits `file-buddy.toml`: the folders to index, the ignore
globs and the four weights. `ctrl-s` saves. A form that does not check out is
not written. New weights apply to the next search, and new folders or globs
start a scan.

## Non-goals for v1

- The application never deletes a file. It only renames.
- No drag out of the window. GPUI has no drag source, and the clipboard does the
  same job in one keystroke.
- No name suggestions from the transcript. Names come from the user.
- No semantic or vector search. No typo tolerance.
- The Settings page edits the TOML file and nothing else. There are no settings
  that live only in the database.
- No moment-level results.

## Stack

| Choice | Decision | Reason |
| --- | --- | --- |
| UI | `gpui-kit` 0.6.1 umbrella, pinned exactly | One version for gpui-pre, gpui-base, gpui-component and the Lucide assets |
| Video | `gpui-video` fork, vendored as project code | The fork must be adapted; upstream merges are not planned |
| Database | `rusqlite` 0.40 with `bundled` | GPUI has no async runtime to share. Zed does the same for local SQLite |
| Media | FFmpeg CLI by subprocess | Already required, decodes every kind, and a crash cannot stop the UI |
| Speech | FFmpeg whisper filter | The vendored FFmpeg build has `--enable-whisper` |
| OCR | `Windows.Media.Ocr` through `windows-rs` | No model files, no extra process. Subject to the spike in issue 01 |
| Clipboard | `clipboard-win` | GPUI cannot put a file on the clipboard |

`gpui-pre` is a snapshot of Zed's main branch, republished weekly by an
automated job that no person reviews. All versions are pinned exactly. An
upgrade is a deliberate task with a build gate.

## Tests

Unit tests on the logic where a mistake is silent: filename tokenization, query
construction, hashing, ffprobe parsing and rename sanitization. One integration
test runs the full pipeline over three fixture files into a temporary database.
No UI tests.

## Build order

See `issues/`. Issues are numbered in the order they should be done.
