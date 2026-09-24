# file-buddy v2 spec

## Purpose

v1 finds a clip by the words you remember. v2 adds two things. It finds a clip
by what it shows, when no words match it. It finds the moment inside a clip.

This spec holds only what changes. The v1 spec at
`.scratch/file-buddy-v1/spec.md` stays the record of everything else. Where the
two disagree, this spec wins.

The facts and measurements behind these decisions are in `research.md`.

## What v2 lifts from the v1 non-goals

- "No semantic or vector search." Lifted. See CLIP.
- "No moment-level results." Lifted in part. A match opens the clip at its time,
  but a result is still a file. The application does not rank moments.

## Worker processes

The models run in worker processes, not in the window. A GPU driver fault or a
runtime crash during an overnight backfill must not close the window.

- A worker is the `fb` binary started with a subcommand. There is one worker
  per model stage: `whisper` and `clip`.
- A worker lives as long as its stage has jobs. It loads its model once and
  takes files over a pipe.
- If a worker stops, the application starts a new one. The job it held loses one
  of its two attempts, the same as any other failure.
- A missing model file stops the stage before any job runs, so no file is marked
  failed for it. The header says which model is missing, and the log has the
  full path. This is how whisper already behaves.
- The CLIP text encoder is the exception. It runs in the window process, on the
  CPU, because search needs it on every keystroke and it is small.

FFmpeg stays the only decoder. A worker never opens a media file. FFmpeg
decodes it and pipes raw samples or pixels to the worker.

## Speech

Whisper moves from the FFmpeg filter to `whisper-rs` with the Vulkan feature, in
the `whisper` worker. The model file stays `ggml-large-v3-turbo.bin`.

The reason is model load. The FFmpeg command line builds its filter graph twice,
so the filter loads the 1.6 GB model twice per file. That is about 2.2 s of the
2.5 s each clip takes. The whole set of 23 test clips holds 532 s of audio,
and transcribing it takes about 10 s. At 4,000 clips, loading costs about 2.4
hours and transcription about 20 minutes. The worker loads the model once.

- FFmpeg decodes the first audio track to 16 kHz mono `f32le` on a pipe.
- The filter already runs on the GPU through Vulkan, so Vulkan is known to work
  on this machine. There is no CUDA build unless Vulkan proves slow.
- `segments.confidence` holds the mean token log probability. `no_speech_prob`
  is recorded too.
- The rule filter from v1 stays as it is. No segment is dropped by a probability
  threshold until the scores have been read over the real collection.
- No word timestamps. A seek to the start of a segment is close enough in a
  short clip.
- No voice activity detection. Silero VAD, v5 and v6, dropped real speech that
  the rules keep, lyrics most of all. See issue 19.
- Every file is transcribed again once with the worker. Files with an edited
  transcript are skipped.

`src/ffmpeg/whisper.rs` goes away.

## CLIP

A fifth signal: what the frames show.

| Choice | Decision |
| --- | --- |
| Runtime | `ort` 2.0.0-rc.13 with `load-dynamic`, and the DirectML execution provider |
| Native library | A pinned DirectML build of `onnxruntime.dll`, vendored in the repository like `ffmpeg/bin`. `build.rs` copies it. The build downloads nothing |
| Model | MobileCLIP2-S2: 512 dimensions, about 405 MB of ONNX files. The user supplies it in `models/`. The application does not download it |
| Preprocessing | Our own code. FFmpeg scales and crops each frame and pipes raw `rgb24`. The `tokenizers` crate handles text |

The `clip` stage embeds the same frames that OCR samples: up to 10 spread over
the duration, and the single image of a still. It is a separate stage with its
own FFmpeg pass, so a failure does not discard the OCR result. A second decode
of a short clip costs about 0.1 s.

Stages: `probe -> thumbnail -> ocr -> clip -> whisper`. CLIP comes before
whisper because it is minutes of work against hours, and both use the GPU. The
5-minute limit applies to `clip` as it does to `ocr` and `whisper`.

The vectors are normalized and stored as BLOBs, one row per frame, with the time
of the frame. The window loads them into memory at start and compares the query
with every vector. At 50,000 vectors this takes about 2 ms. There is no vector
index and no `sqlite-vec`.

## Search

The FTS5 query does not change. Its four bm25 weights still rank one list.

CLIP gives a second list:

1. The text encoder embeds the query.
2. Each file scores by its best frame.
3. A file stays only if its score is at least `max(floor, top - delta)`, where
   `top` is the best score of the query. CLIP always returns a best match, even
   for a query that nothing fits, and this cutoff is what stops that.

The two lists merge by weighted reciprocal rank fusion, with `k = 60`. The CLIP
weight is lower than the FTS weight, so a word that is in a filename or a tag
still puts that file first, and visual hits fill in below and between the text
matches. The CLIP weight is a fifth weight on the Settings page. `floor` and
`delta` are constants until measurement says otherwise.

- FTS results show at once. The visual hits merge in after a debounce of about
  150 ms, which is the time the text encoder needs. If the reordering is hard
  to follow, the fallback is to wait for both and show one list.
- A `~` at the start of the query searches visually only: `~cat in a box`.
- The chips and the Folders scope limit visual results the same as text results.
- A visual match has the badge `visual`.

## More like this

`a` replaces the results with the clips that look like the selection. It was
`l` until issue 25 gave `l` to the rows.

- The query is the mean of the selection's frame vectors. With files marked, it
  is the mean over all the marked files.
- Each file scores by its best frame. The query files are left out.
- The 50 best files show, after the chips and the Folders scope. There is no
  cutoff by score: between two files, scores run from about 0.6 to 0.97 and
  spread differently for each file. See issue 22.
- The search box shows `like: <name>`, or `like: <n> files`. Typing over it
  ends the search. Escape goes back to the results from before, with their
  selection and marks, after it clears the marks. `a` again goes one step
  deeper, and each escape goes back one step.
- The chips still filter.

## Moments

A speech, OCR or visual match carries the time where it matched.

- The side pane opens the clip at that time.
- The tile's snippet starts with the time: `0:42 the line you said`. A visual
  match shows the time with no text.
- The Speech tab lists segments, and the Text tab lists lines, each with its
  time. A click on a time seeks to it.

The `ocr` table changes from one text per file to one row per line, with the
time of the frame where the line first shows. This is a migration and one OCR
pass over every file, which takes minutes. It is done as part of that issue,
not before.

## Editing speech and screen text

The Speech and Text tabs can edit, delete and add a segment or a line. A change
goes to the index at once.

- Each row has an `edited` flag. A deleted row stays, with empty text, so a
  later run does not bring it back.
- A file with an edited row skips any new run of that stage.
- Edits are user data that the application cannot rebuild, like tags.
  Migrations must never drop them.
- An edited OCR line is always indexed, even if the watermark rule would leave
  it out. The user typed it on purpose.
- A tab can be edited only after its stage has run for the file. Before that,
  an edit would stop the stage from ever running.
- `i` cycles the tabs: File, Speech, Text. `e` puts a cursor on the rows of the
  Speech or Text tab, on the row the clip is at. In the rows, up and down move
  the cursor and seek the clip to the row. Enter edits the row, with its text
  selected: enter saves, and escape cancels. Enter on a tab with no rows adds
  one. `delete` deletes the row. `l` adds a row at the time of the clip. A
  double click on a row edits it.
- Escape, after the tag panel and the rename input: it closes the row input,
  then takes the cursor off the rows.
- Focus that leaves the rows or their input, such as a click outside them, ends
  the editing and drops what was typed.

## Preview controls

The side pane gets controls. Tile videos do not change.

- A thin progress bar under the clip, with the time and the length beside it.
  A click seeks, a drag scrubs, and hovering shows the time under the pointer.
- `shift-left` and `shift-right` skip back and forward 5 s.
- Seeks are accurate where they are fast enough: a skip, a click on the bar and
  the end of a drag land on the exact time. During a drag, seeks go to
  keyframes.
- Space and a mouse press on the clip share two jobs. A tap shorter than about
  250 ms toggles pause on release. A longer press plays at 2x while held, and
  on release the clip goes back to its state from before. The sound follows
  the speed if `gpui-video` keeps it clean, and is muted during the hold if not.

## History

The mouse back and forward buttons, and `alt-left` and `alt-right`, move
through a history, as in a browser.

- A page switch, a folder change, a Review group and "more like this" each make
  a step. A step saves the state the user leaves: page, folder, query, Review
  group, selection and scroll position. Typing makes no step.
- The chips, the sort and the view are not part of a step.
- A new step after going back drops the forward steps. At most 100 steps, in
  memory only. A folder that no longer exists is skipped.
- Back and forward close an open panel or input and navigate at once.

## Custom keys

The window keys can change. The keys in the v1 spec are the defaults.

A `[keys]` table in `file-buddy.toml` holds only the changes, keyed by action.
Each action takes a list, so it can have several keys, and an empty list
unbinds it:

```toml
[keys]
rename_file = ["r", "f2"]
toggle_table = []
```

A new default key in a later version reaches the user unless they have
overridden that action.

These keys are fixed. They cannot be removed, given to another action, or added
to:

| Key | Action |
| --- | --- |
| arrows | select left, right, up, down |
| `enter` | activate |
| `escape` | dismiss |
| `backspace` | up one folder |
| `ctrl-c` | copy file |
| `ctrl-s` | save settings |
| `ctrl-1` to `ctrl-4` | switch pages |
| `alt-up`, `alt-down` | tag suggestions |
| mouse back and forward, `alt-left`, `alt-right` | history |

The keys inside the text inputs belong to gpui-component and do not change.

- **Context comes from the key.** A key with `ctrl` or `alt`, or an F key, works
  everywhere. Any other key works only outside the text inputs, so it never
  takes a letter the user types. `shift` alone does not count, because `shift-a`
  types `A`. The user never sees GPUI contexts.
- **Conflicts.** Two actions on one key, or a key that a fixed action holds,
  stop the save. The error names both actions.
- **No key sequences.** Only single keys with modifiers.
- **Settings.** A Keys section lists each action that can change, with its
  keys. Up or down on the page, outside a field, goes into the list. Enter
  captures the next key and adds it. `delete` removes the last key
  of the row. `r` resets the row to the default. Escape cancels a capture. A row
  that differs from the default is marked. `ctrl-s` saves, as for the rest of
  the page.
- **A bad entry.** An unknown action or a key GPUI cannot parse is skipped. The
  other entries apply. The header and the Keys section name what was skipped.
- **When keys apply.** On start and on save, with no restart. The application
  clears all bindings, then binds gpui-component's keys, the fixed keys, the
  defaults and the overrides again. `clear_key_bindings` also removes
  gpui-component's keys, so the first bind keeps a copy of them to bind again.
- **A key must be a key.** GPUI takes any word as a key name, and a key named
  `blah` never fires. A key is a single character, an F key or a named key
  such as `space` or `pageup`, with modifiers.

## FFmpeg

- Software decode stays for every call. Hardware decode was measured and was 3
  to 4 times slower on these clips, because each call sets up a GPU device for a
  few seconds of video.
- The `fps` filter stays for frame sampling. Seeking with `-ss` wins only on
  the longest clip.
- Every call still passes `-nostdin` and closes standard input.
- Playback in `gpui-video` has no hardware decode. If a large clip ever
  stutters or loads the CPU, D3D11VA there is the place to start. Every frame
  must still come back to CPU memory, because on Windows GPUI paints only
  through `paint_image`. Measure first.

## Non-goals for v2

- Trimming, converting or re-encoding a clip for export. Planned for v3.
- Near-duplicate detection. Exact duplicates by hash stay.
- Typo tolerance.
- Tag suggestions from CLIP. The tags are too specific for the text encoder.
- Topic clusters on Browse.
- Search by sound (CLAP).
- Word-level timestamps.
- Whisper on ONNX Runtime. See `research.md`.
- Hardware decode.
- Indexing files longer than 5 minutes. The limit stays.
- Dropping speech by a probability threshold, until the scores are read.
- Voice activity detection.

## Stack changes

| Choice | Decision | Reason |
| --- | --- | --- |
| Speech | `whisper-rs` 0.16.0, Vulkan feature, pinned exactly | The model loads once per worker, not twice per file. Probabilities come with it |
| Build tools | CMake, LLVM (clang) and the Vulkan SDK | `whisper-rs` builds whisper.cpp from source |
| CLIP runtime | `ort` 2.0.0-rc.13, `load-dynamic`, DirectML | No CUDA install. The build downloads nothing |
| Tokenizer | `tokenizers`, pinned exactly | The CLIP text side |

## Tests

Unit tests on the new logic where a mistake is silent: the RRF merge and the
cutoff, the `~` and `like:` query forms, vector BLOB round trips, the mean of
several files, the time format on a tile, the `edited` rule for re-runs, and
the `[keys]` rules: context from the key, conflicts, fixed keys and skipped
entries.
The pipeline integration test gains a `clip` stage. It needs the model in
`models/`, as the speech test does.

## Build order

Issues continue from v1, in `issues/`:

| Issue | Work |
| --- | --- |
| 18 | whisper-rs worker, and the worker process that CLIP reuses |
| 19 | VAD measurement (rejected), then the full re-transcription |
| 20 | CLIP stage and storage |
| 21 | Search merge and `~` |
| 22 | More like this |
| 23 | Moments |
| 24 | OCR lines with times |
| 25 | Editing speech and screen text |
| 26 | Custom keys. It touches no model work and can be built at any time |
| 27 | Middle-click autoscroll. A spike first, and it can be built at any time |
| 28 | Browse the whole collection, and sort it with no query. It can be built at any time |
| 29 | Preview seek bar, skip and click to pause. Build it before 23, which needs accurate seeks |
| 30 | Hold for 2x speed |
| 31 | Back and forward history |
