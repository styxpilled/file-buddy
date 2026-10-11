# file-buddy v3 spec

## Purpose

v1 and v2 find a clip. v3 makes something from it. A Studio page cuts a range
from a source, crops it, scales it, changes its speed and puts text on it. The
result goes out as MP4, WebM, GIF or animated WebP, and lands on the clipboard
ready to paste. A screenshot key copies the full-size frame as an image.

For a GIF or an animated WebP, the Studio edits the frames one by one. It also
makes a GIF or a WebP from a set of images.

v3 also makes animated WebP work. Today it fails at every stage. Files dropped
from Explorer join the index with no root. Smaller changes: a context menu on
each result, a ctrl-click on a kind chip, and escape clears the query.

This spec holds only what changes. The v1 and v2 specs stay the record of
everything else. Where they disagree, this spec wins. The measurements behind
these decisions are in `research.md`.

## What v3 lifts from the earlier non-goals

- "Trimming, converting or re-encoding a clip for export." Lifted. See Studio.
- "FFmpeg stays the only decoder." Lifted for animated WebP only. See Animated
  WebP.

## Animated WebP

The FFmpeg 8.1.2 `webp` decoder skips the `ANIM` and `ANMF` chunks. An animated
WebP probes as 0x0, and the thumbnail, OCR and CLIP stages fail with "no
packets". Playback fails for the same reason. FFmpeg cannot read back the
animated WebP that it writes, so WebP exports would fail too.

- The application finds an animated WebP from its header: a `VP8X` chunk with
  the animation flag. It does not ask FFmpeg. `image-webp` uses the same flag,
  so a file without it could not be decoded either way.
- `image-webp` decodes it. The crate is already in the dependency tree at 0.2.4.
  Pin it exactly as a direct dependency.
- The probe takes the size, the frame count and the duration from the decoder.
- The probe also writes a proxy: the frames go to FFmpeg as raw RGB on a pipe,
  and FFmpeg encodes them to H.264 at CRF 12, beside the thumbnail, as
  `<hash>.proxy.mp4`. Frames hold their own durations in milliseconds. The
  proxy keeps them by repeating frames at a constant rate: the largest step
  that divides every delay, and never under 10 ms.
- A stage that finds no proxy for an animated WebP makes it, so a cleared cache
  costs no failed job.
- The thumbnail, OCR and CLIP stages, the tiles, the side pane and the Studio
  all read the proxy in place of the file. They need no other change.
- Transparent pixels go on black in the proxy.
- The clipboard, rename, delete and Explorer actions still act on the real
  file.
- A file that fails to decode is recorded with its error, as FFmpeg failures
  are.

## Studio

A fifth page. `c` opens the selection in the Studio. `ctrl-5` switches to the
page. Opening a source makes a history step.

The Studio holds one edit. Its source is a video, an animated file, a still or a
sequence of stills. The edit stays while the window is open, until another
source replaces it. Export does not clear it, so the same
edit can go out as a GIF and then as an MP4. Nothing goes to the database and
no project files exist.

The page has a large preview, a timeline under it and a panel of controls
beside it. The preview shows the source with the edit applied: crop, text and
speed. Scale and quality show only in the export.

### Range

One in point and one out point per edit. The timeline holds the seek bar of v2
with two handles on it.

- `[` sets the in point at the playhead and `]` sets the out point.
- A drag on a handle moves it, and the preview seeks to it.
- Playback loops over the range.
- The panel shows the length of the range, after the speed change.

### Crop

A box on the preview. Outside it, the preview goes dark.

- Drag the box to move it, and drag a corner or an edge to resize it.
- The aspect is free, source, 1:1, 16:9 or 9:16.
- The output dimensions are even, because H.264 needs that.

### Scale

A maximum width: source, 1080, 720, 480, 360, or a number. The output never
grows larger than the crop.

### Speed

Presets from 0.5x to 4x. The preview plays at the speed. Video exports keep
their sound, and `atempo` keeps the pitch.

### Text

Each edit holds any number of texts. A text has its words, a position, a size,
a time range and an optional black box behind it.

- `n` adds a text at the center of the frame, for the whole range.
- A click on a text in the preview selects it, and a drag moves it. Ctrl and
  the wheel over it change its size.
- Each text has a row in the panel, with its size in px, a Box toggle and
  Delete. A double click edits its words. `shift-enter` adds a line.
- Each text has a row on the timeline under the range. The row shows a bar
  from its start to its end, and a drag on either end changes it.
- `delete` removes the selected text, when the text is selected and no input
  has focus.
- The font is fixed: Arial Bold, `C:/Windows/Fonts/arialbd.ttf`. The color is
  white. The box is black, with padding.
- The position and the size are fractions of the output frame, so a change of
  crop or scale keeps the text in place.
- The preview draws the text in GPUI with the same font file. The export
  burns it in with `drawtext`. The two must match to within a few pixels.

### Captions

Add captions turns the speech segments inside the range into texts. The
captions read as plain subtitles: one segment at a time, a full line, and no
word-by-word highlight.

- Each segment becomes one text, with the segment's start and end in output
  time. A segment that crosses an end of the range is cut to it.
- Edited segments apply. Deleted segments do not show.
- The texts sit at the bottom center, at one size, with the box on. After
  that they are ordinary texts: each can move, change or go.
- A long segment wraps to at most two lines that fit 90% of the frame width.
  The application breaks the lines, because `drawtext` does not wrap.
- The command does nothing for a file whose speech stage has not run, and the
  header says so.

### Stills

A still opens in the Studio with no timeline and no speed. Crop, scale and text
apply. Text has no time range. The export is PNG or JPG.

### Frames

A GIF or an animated WebP opens in frame mode. A strip of every frame, each
with its delay, sits over the range.

- A click selects a frame and the preview shows it. Shift-click selects a
  run, and ctrl-click adds or removes one frame.
- `delete` removes the selected frames. `ctrl-d` duplicates them, after the
  last of them.
- A drag moves the selected frames to a new place in the strip.
- The panel edits the delay of the selected frames, in milliseconds.
- Space plays the frames with their delays, over the range in a loop.
- The range is whole frames. `[` and `]` take the frame on the stage, and a
  drag on a handle snaps to the nearest gap between frames. The export keeps
  the frames in the range.
- `s` skips the selected frames, or plays them again. A skipped frame stays
  on the strip, faint, but has no time: it does not play and does not export.
  One frame always plays.
- `g` shows a grid of every frame, each with a Skip and a Copy button. Copy
  puts the frame on the clipboard as `p` does, with the crop and the texts.
  A drag in the grid moves the selected frames, as on the strip.
  The grid takes the room the letterbox leaves: above the stage for a frame
  wider than the stage, beside it for a narrower one, and from 30% to 60% of
  the room.
- Crop, scale, speed and text still apply. Text times are in output time.
- `image` decodes a GIF and `image-webp` decodes a WebP, into memory, on a
  background thread. The edit opens with the first frame, and the others join
  the strip as they decode. The export waits for the last. The frames are
  full RGBA images, with each delay as the file gives it. Every frame stays in
  memory while the edit is open, with no limit and no cache. This machine has
  the memory for it.
- While the frames play, the window draws when the next frame is due, not at
  the rate of the display.

### Image sequence

With two or more stills marked, `c` opens them as a sequence of frames, in name
order. The frame strip works as it does for an animated file. The canvas and
the place of each frame below apply to an animated file too.

- The canvas size starts at the size of the first image, and the panel
  changes it.
- Fit or fill applies to every frame: fit scales each image to show all of
  it, with transparent bars, and fill scales it to cover the canvas, centered.
  MP4 and WebM make the bars black.
- Each frame then has its own offset and scale on top of that. While the
  frames do not play, a drag on the preview moves the selected frame, and an
  edge near an edge of the canvas snaps to it. Ctrl and the wheel scale it.
  Reset puts it back to the fit or fill. The frames do not play during a drag.
- While a frame is selected, the frame before it shows faint over it, so the
  two can line up.
- Each frame starts with a delay of 100 ms.

## Export

`ctrl-enter` exports the edit.

| Format | Encoder | Audio | Controls |
| --- | --- | --- | --- |
| MP4 | `h264_nvenc` | AAC | quality |
| WebM | `av1_nvenc` | Opus | quality |
| GIF | `gif`, with `palettegen` and `paletteuse` | none | fps |
| Animated WebP | `libwebp_anim` | none | fps, quality |
| PNG, JPG | `png`, `mjpeg` | none | JPG quality |

- WebM uses AV1, because `av1_nvenc` is in the FFmpeg build and the RTX 4070
  SUPER encodes it. This costs no new dependency and no new code path. Where
  an application cannot play AV1, the user exports MP4. There is no VP9.
- The controls are direct. There is no size target. The header shows the size
  of the file after the export.
- The filter order is trim, crop, scale, text, speed. One function builds the
  FFmpeg arguments from the edit. It has no side effects, so it can be
  tested.
- The export runs as an FFmpeg subprocess and the window does not wait. The
  page shows its progress. One export runs at a time.
- When the export ends, the file goes to the clipboard as `CF_HDROP` and as
  path text, as Enter does. The header confirms it.
- The name is `<source stem> <in>-<out>.<ext>`, for example
  `cat jump 0m03-0m07.mp4`. A still has no range: `<source stem>.<ext>`. A
  sequence is `<first stem> sequence.<ext>`. A name that exists gets ` (2)`,
  ` (3)`, and so on.
- A failed export keeps the FFmpeg error in the log, and the header says the
  export failed.
- In frame mode, each frame keeps its own delay. FFmpeg gets each frame with
  its duration, not at a constant rate, and the fps control does not show. A
  constant rate fine enough for every delay breaks a GIF, because browsers
  play a delay of 10 ms or less at 100 ms.

### Tags carry over

An export gets the tags of its source. Tags belong to the hash, so the export
writes them for the hash of the new file as soon as the file is written,
before any scan finds it. A sequence export gets the tags of every still in
it.

### Export folder

`export_folder` in `file-buddy.toml`, edited on the Settings page. With no
folder set, export does nothing and the header says to set one.

The user makes the export folder a root, so exports are indexed as any other
file. The code does not treat the folder in a special way.

## Screenshot

`p` copies the current frame at full size to the clipboard as an image. No file
is written.

- In the side pane, the frame is the raw source frame at its own resolution.
- In the Studio, the frame has the crop and the text, at the resolution of the
  crop, with no scale.
- FFmpeg extracts the frame at the exact time to PNG on a pipe. The clipboard
  gets `CF_DIB` and the registered `PNG` format.
- The header confirms the copy.

## Dropped files

Files and folders dropped from Explorer on the window join the index. GPUI
already takes the drop on Windows, as `ExternalPaths`.

- A dropped file goes through every stage, as a file in a root does.
- A dropped folder adds the media files in it and in every folder under it,
  once. The folder is not a root. The watcher does not watch it, and a later
  file in it does not join.
- A file under a root is a root file, dropped or not. A file outside every root
  is a dropped file. The `files` table records this. If a later root covers a
  dropped file, it becomes a root file.
- A scan does not mark a dropped file missing because no root holds it. The
  scan checks that the file is still on disk.
- The roots list of the Folders page has a Dropped entry after the roots,
  while any dropped file exists. It lists the dropped files.
- After a drop, the window opens the Dropped entry and selects the first
  dropped file. The header says how many files joined. A file that is
  already in the index does not join again.
- Rename, trash and delete work on a dropped file as on any file.

### Paste

`ctrl-v` outside the text inputs adds what the clipboard holds.

- Files, as `CF_HDROP`: the same as a drop.
- An image, as the registered `PNG` format or `CF_DIB`: the application saves
  it to the export folder as `pasted <date> <time>.png`. The export folder is
  a root, so the image is a root file. The window selects it when the scan
  finds it. With no export folder set, the header says to set one.
- `ctrl-v` in a text input pastes text as before. This is the one key with
  `ctrl` that does not work everywhere.

### Delete from database

This removes a dropped file from the index and leaves it on disk.

- It works only on dropped files. A root file would come back on the next scan.
  With files marked, it skips the root files among them, and the header says
  how many it skipped.
- The file leaves the index as a file the watcher finds gone does, and then
  its row goes. Its tags stay with its bytes, as for a deleted file.
- It has no default key. The action is `forget_file`, so a key can be given in
  `[keys]`.

## Context menu

A right click on a tile or a table row opens a menu of file actions.

- If the file is marked, the menu acts on all the marked files. If not, the
  click selects the file first, and the menu acts on it.
- Copy, Reveal, Rename, Open, Open in Studio, More like this, Tag, then Trash,
  Delete and Delete from database. Each item shows its key.
- An item that needs one file is disabled when the menu acts on several.
  Delete from database is disabled when none of the files is dropped.
- The side pane's file menu is identical. It gains Open in Studio, More like
  this, Tag and Delete from database. One list builds both menus.

## Kind chips

A ctrl-click on a chip turns that kind on and the others off. A ctrl-click on
the only chip that is on turns every kind on again.

## Escape clears the query

Escape gets one more step, at the end of the order: when nothing else is left
to undo and the search box holds text, it clears the box. It works with focus
outside the box. Like typing, it makes no history step.

The order is now: middle-click autoscroll, the tag panel, the rename input, the row input, the rows, a
Settings field, one step back in history from the Studio, back to Browse from
Review or Settings, focus out of the search
box, the marks, one step out of "more like this", then the query.

## Side pane width

The side pane's minimum width is 260 px. That is too narrow for its top row:
Autoplay, Play, Mute, the speed, the sound level and the file menu. The
minimum becomes the width of that row at its normal size, so the row never
wraps or clips. Measure the row in the running window, and do not guess the
number.

## Keys

New default keys, all of which can change through `[keys]`:

| Key | Action |
| --- | --- |
| `c` | open in Studio |
| `p` | screenshot |
| `[`, `]` | set in point, set out point |
| `n` | add text |
| `ctrl-enter` | export |
| `ctrl-d` | duplicate frames |
| `s` | skip frames |
| `g` | frame grid |
| none | delete from database, `forget_file` |

In the Studio, `delete` removes the selected text or frames. The Studio is a
context inside the window, as the rows of the Speech and Text tabs are, so its
keys win there.

`ctrl-5` joins the fixed keys, with `ctrl-1` to `ctrl-4`. `ctrl-v` joins them
too, outside the text inputs only.

## Non-goals for v3

- More than one range per export. Joining videos. A sequence takes only
  stills.
- Per-frame edits on a video.
- Watching a dropped folder.
- A file size target.
- Font choice, text color and outline.
- Saved projects, or an edit that comes back after a restart.
- A muted export. Sound always follows the video.
- Filters, color changes, rotation and transitions.
- A screenshot saved as a file.
- Animated WebP through FFmpeg. The proxy stays until a release of FFmpeg
  decodes it.

## Stack changes

| Choice | Decision | Reason |
| --- | --- | --- |
| Animated WebP | `image-webp` 0.2.4, pinned exactly | It is already in the tree. FFmpeg cannot decode animated WebP |
| GIF frames | the `gif` feature of `image` 0.25.10 | Frames with their delays. The `gif` crate is already in the tree |

The FFmpeg build already has every encoder and filter that v3 needs.

## Tests

Unit tests on the logic where a mistake is silent: the FFmpeg arguments built
from an edit, for each format; the text position and size math after a crop
and a scale; even output dimensions; the export name and its collision suffix;
the RIFF check for animated WebP; the frame durations to a constant rate; frame
delete, duplicate and move with their delays; the fit and fill math with an
offset and a scale; root or dropped for a path; the skip rule of delete from
database; the chip ctrl-click; the escape order; captions from segments, with
the cut at the range ends and the line wrap; and the tags an export gets.

One integration test exports a fixture in each format and probes the results.
It needs the GPU for NVENC.

## Build order

Issues continue from v2, in `issues/`:

| Issue | Work |
| --- | --- |
| 35 | Animated WebP: detection, proxy, indexing and playback |
| 36 | Export folder setting, and the Studio page with range, MP4 export and clipboard |
| 37 | WebM, GIF and animated WebP export, with their controls. Needs 35 to index WebP exports |
| 38 | Crop and scale |
| 39 | Speed |
| 40 | Text: drag, size, box and time range |
| 41 | Stills in the Studio |
| 42 | Screenshot. It can be built at any time |
| 43 | Frame mode for GIF and animated WebP |
| 44 | Image sequence. Needs 43 |
| 45 | Dropped files, and delete from database |
| 46 | Context menu. Needs 36 for Open in Studio and 45 for Delete from database |
| 47 | Ctrl-click on a kind chip. It can be built at any time |
| 48 | Escape clears the query. It can be built at any time |
| 49 | Side pane minimum width. It can be built at any time |
| 50 | Captions from the transcript. Needs 40 |
| 51 | Tags carry over to exports. Needs 36 |
| 52 | Paste to add. Needs 45 |
| 53 | Simulated input for the dev window. Tooling, not a feature. Build it before the Studio work, which needs many window checks |
| 54 | Playback faults: sound and picture drift after scrubbing, a seek that shows the first frame, and GIFs that show no frame |
| 55 | A drag of the out handle stops playback in the Studio |
| 56 | A design system based on Adobe Spectrum, around the accent `#30a7fe` |

## Maybe

Not planned. Kept here in case it proves useful.

- Find a loop. A command moves the out point to the frame that looks most like
  the in point, so a GIF loops with no jump. It compares the pixels of frames
  near the out point, with no model. It is neat, but a cut by eye does the
  same job most of the time.

## Open questions

- Does the GPUI text match `drawtext` to within a few pixels at the same size
  with the same font? If it does not, the fallback is to render each text to an
  RGBA image in the application and burn it in with `overlay`. Then the
  preview and the export use the same pixels.
