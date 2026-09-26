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

### 2026-09-26: measurements

Tools: `scripts/memory.ps1` samples private bytes, working set, GPU memory and
handles of each process to a CSV. VMMap splits the memory by kind. dhat-rs,
behind the `dhat-heap` feature, gives the Rust heap by call site. All runs are
the debug build on 3,417 files, 31,628 vectors, RTX 4070 SUPER, 100 % scale.

Only `file-buddy.exe` grows. The `fb` workers run only while their stage has
jobs, then exit.

| Action | Private MB | Working set MB | GPU dedicated MB |
| --- | --- | --- | --- |
| Idle, 150 s | 420 -> 420 | 364 -> 364 | 78 -> 78 |
| Grid, scroll to the bottom and back | 419 -> 2,343 | 363 -> 1,069 | 78 -> 1,287 |
| Table, scroll to the bottom | 453 -> 441 | 383 -> 380 | 72 -> 63 |
| 100 typed queries, table | 475 -> 482 | 407 -> 401 | 65 -> 103 |
| 100 `~` queries, table | 474 -> 820 | 406 -> 456 | 65 -> 382 |
| 120 selection moves, table | 474 -> 627 | 406 -> 406 | 65 -> 214 |
| 128 hovers over the same 16 tiles | 475 -> 851 | 407 -> 397 | 65 -> 440 |

Nothing of it comes back while the window stays open.

**Idle, 458 MB private (VMMap).** 310 MB heap, 91 MB private data, 52 MB
images. dhat puts the Rust heap at 109 MB, 65 MB of it the vectors. The other
~200 MB of heap, and most of the 162 MB of mapped files, are the text encoder:
`text.onnx.data` is 254 MB of weights. This is a fixed cost, not growth.

**Cause 1: thumbnails are never freed.** `tile::thumbnail` calls `img(path)`
with no image cache, so GPUI keeps every decoded image in `App::loading_assets`
for the life of the window, and its texture in the sprite atlas. After a full
scroll VMMap shows the heap at 1,014 MB (+700 MB, all resident) and private data
at 1,167 MB (+1,076 MB, 11 MB resident: GPU allocations). That is about 0.2 MB
on the CPU and 0.35 MB on the GPU per thumbnail.

**Cause 2: each video frame texture leaks when its element goes.**
`gpui-video`'s `VideoElement::cached_image` keeps the last frame in
`use_state` and calls `drop_image` on it only when a newer frame replaces it.
When the element leaves the tree (hover ends, the pane shows the thumbnail
between clips), GPUI drops the state, the `Arc<RenderImage>` goes, and the
atlas texture stays. About 1.3 to 3 MB of GPU memory per clip shown, the size
of one frame. The working set stays flat, so the CPU copy is freed.

**Memory hogs that do not grow.** Measured at idle.

| What | Memory | Note |
| --- | --- | --- |
| CLIP text encoder, in the window | ~250 MB, heap and mapped file | `text.onnx.data` is 254 MB of fp32 weights. An fp16 export would halve it |
| CLIP vectors | 65 MB, Rust heap | 31,628 × 512 floats, as the spec says |
| Other Rust heap | ~45 MB | 15 MB decoded images on screen, 6 MB video frames, the tokenizer, GPUI |
| Images (DLLs) | 52 MB private | ONNX Runtime, FFmpeg, D3D |
| `whisper` worker | ~1.6 GB of GPU memory | Only while its stage has jobs, then the worker exits |

### 2026-09-26: fixes

1. `gpui-video` keeps the frame of each video element in a global, and
   `gpui_video::release_unused`, called at the start of every render, frees
   the frames of the elements that were not painted in the last frame.
2. `ui::thumbnails::Thumbnails` is an `ImageCache` for the tiles and the pane.
   It keeps at most 200 thumbnails and drops the least recently shown with
   `drop_image`, which frees the CPU copy and the atlas texture. A thumbnail
   costs about 0.5 MB on the CPU and 0.8 MB of GPU memory, more than the first
   estimate, so 400 held +600 MB.

| Action | Before (private / GPU MB) | After (private / GPU MB) |
| --- | --- | --- |
| Grid, two full scrolls down and back | 2,343 / 1,287 after one | 757 / 239 |
| 128 hovers over the same 16 tiles | 851 / 440 | 438 / 53 |
| 120 selection moves | 627 / 214 | 553 / 139 |

The grid levels off: three passes at capacity 400 stayed between 1,040 and
1,160 MB. Hover returns to where it started. Selection still grew at 320 moves
(465 -> 727 MB, GPU 65 -> 255), which is about what 200 pane thumbnails cost,
but a plateau was not confirmed. Check that first if the memory still seems to
grow.

3. Tiles show a second, smaller thumbnail beside the full one,
   `<hash>.tile.jpg`, that fits a 432 x 240 box, twice the picture in a tile.
   The thumbnail stage makes it, and the window makes a missing one when it
   first shows the tile. The pane keeps the 480 px file. Two full grid
   scrolls: 540 MB private, 120 MB GPU. A version with only the small file,
   in the pane too, looked worse there and was dropped.

The scan never made a missing thumbnail again: `jobs::enqueue` is
`INSERT OR IGNORE`, and the done job stopped it. `jobs::redo` now queues a done
job again, and leaves a failed one alone.
