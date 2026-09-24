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
3. Compare with commit `a6e4afc`, before v2, for the same actions. If the growth
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
