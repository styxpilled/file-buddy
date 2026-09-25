# 32 - Memory growth in the window

Status: needs-triage

Depends on nothing. Found by the user on 2026-09-24, in short testing after
issue 21. The memory of the application seems to grow. It is not known yet
which process grows, what makes it grow, or if 20 and 21 caused it.

## Find the cause first

Measure before any fix.

1. Find which process grows: `file-buddy.exe`, or one of the `fb.exe` worker
   processes. Record private bytes and working set of each over time.
2. Find the action that makes it grow. Measure each alone for some minutes:
   - the window with nothing to do
   - typing queries, with and without the visual weight at 0
   - `~` queries only
   - moving the selection, so the side pane plays one clip after another
   - hover videos on the tiles
   - a stage that runs while the window is open (OCR, CLIP, speech)
     is there too, 20 and 21 did not cause it.

## Places to look

Only suspects. None is confirmed.

- `ui::visual`: `refresh_vectors` uses `Arc::make_mut`. If a visual search
  holds the vectors when new rows arrive, this copies all of them, about 64 MB
  at 31,514 vectors. While the `clip` stage runs, the window refreshes the
  results often. A copy should be freed when the search ends, so this is a peak
  and not a leak, unless a task keeps its copy alive.
- `clip::text::TextEncoder` in the window: ONNX Runtime keeps a CPU memory
  arena for each session, and it can grow with input shapes. The input is
  always 77 tokens, so it should stay the same size.
- The query vector cache holds at most 64 vectors of 512 floats. It is small
  by design.
- `gpui-video`: decoded frames for the side pane and the tile videos. Growth
  here would be there before v2 too.
- `whisper-rs` leaks a small string for each `set_language` call, one per
  file. This is in the `whisper` worker, not the window, and the worker stops
  when its stage ends.

## Done when

The process and the action that grow are known, with numbers, and the cause is
fixed or written up here with the reason it is not a real problem at the scale
of the collection.

## Comments

### 2026-09-25: thumbnails (user note)

After issue 28, memory grew a lot. Browse now lists all 3427 files, and a
scroll through the list loads each thumbnail. The user thinks the thumbnails
cause it. Investigate the memory as a whole, and also look at the thumbnails
in particular.

What is known now, not measured:

- `ffmpeg::thumbnail` writes a JPEG with the long edge at most 480 px
  (`MAX_EDGE`). The cache holds 3375 of them, 65 MB on disk.
- A tile shows the picture in at most 216 x 120 px (`TILE_WIDTH - 4`,
  `TILE_HEIGHT - 4`).
- Decoded as RGBA, a 480 x 270 thumbnail is about 0.5 MB. If the window keeps
  every decoded thumbnail, one scroll through the whole list holds about
  1.7 GB. At 216 x 120 it is about 0.1 MB each, about 0.35 GB for all.
- `tile::thumbnail` uses `img(path)`. Find out if GPUI keeps each decoded image
  in its asset cache for the life of the window, and if it frees any of them.

Ideas from the user:

- Make the thumbnail files smaller, because the tiles are small. Check the
  display scale first: at 150 % or 200 % a tile needs more pixels. The side
  pane also shows the thumbnail (`preview.rs`, while no clip plays), at the
  full width of the pane, so it needs the large one. Two sizes, or the pane
  reads something else. A new size needs the old files made again, or made
  again when they are next shown.
- Drop decoded thumbnails that have not been shown for some time, so that
  memory stays flat on a long scroll.

Measure memory before and after a scroll from the top to the bottom of Browse,
in the grid and in the table. The table shows no thumbnails, so it is the
control.
