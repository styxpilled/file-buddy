# 43 - Frame mode for GIF and animated WebP

Status: done

Depends on 35 and 37. Read the Frames section and the frame mode part of
Export in the v3 spec first.

## Work

- A GIF or an animated WebP opens in frame mode. A strip of frames with their
  delays takes the place of the range.
- Decode with `image`, with its `gif` feature, and with `image-webp`. Every
  frame stays in memory as full RGBA while the edit is open. No limit, no
  cache.
- Click selects, shift-click selects a run, ctrl-click adds or removes one.
- `delete` removes the selected frames. `ctrl-d` duplicates them after the
  last of them. A drag moves them.
- The panel edits the delay of the selected frames in milliseconds.
- Space plays the frames with their delays.
- Crop, scale, speed and text still apply. Text times are in output time.
- Export keeps each frame's own delay. FFmpeg gets each frame with its
  duration, for example through the concat demuxer, not at a constant rate.
  The fps control does not show. Browsers play a GIF delay of 10 ms or less at
  100 ms, so a fine constant rate breaks GIF output.

## Tests

Delete, duplicate and move, each with the delays. The durations FFmpeg gets.

## Check

Cut frames from a GIF and export it as GIF and as WebP. Both play in a browser
with the original timing.

## Comments

### 2026-10-11

Built. The user played the GIF and WebP exports in a browser on 2026-10-11, and both are correct.

- A GIF or an animated WebP opens in frame mode. The frames decode on a
  background thread. An animated PNG still opens as a video, because the
  issue names only GIF and WebP.
- A delay of 10 ms or less reads as 100 ms, as a browser plays it. The panel
  takes 20 ms or more. GIF and WebP exports keep each delay at 20 ms or more
  after the speed, for the same reason.
- The strip shows each frame with its delay. A press on a selected frame
  waits for the release, so a drag moves the whole selection. A frame and a
  text are never selected together, so `delete` has one target.
- Export writes each image once as PNG to a temporary directory, with an
  `ffconcat` list at `option framerate 1000`, so each delay is exact to the
  millisecond. The speed goes into the delays, with no `setpts` and no `fps`.
- The last delay needs its own rule in each format. GIF takes
  `-final_delay`. MP4 and WebM end on the last image once more. libwebp gives
  the last frame the mean of the others, so the export sets the last `ANMF`
  delay after FFmpeg ends. MP4 and WebM use `-bf 0`, because B-frames gave
  the frames the wrong times.
- Checked in the sim build on a 49-frame GIF: select, shift-click, a
  300 ms delay, duplicate, a drag, delete, a text, and export as GIF and
  WebP. Both exports decode to 49 frames and 5700 ms with the four 300 ms
  frames in place. The GIF matches the source pixels.

### 2026-10-11, second pass

The user asked for five changes after use: the fans ran high while a GIF
played, the strip cut its pictures, a GIF had no range, the frames took long to
open, and they wanted a grid of every frame.

- Playback no longer asks for a draw at the rate of the display. A timer draws
  the window when the next frame is due, and the seek bar reads the start of
  the frame under the playhead, so a draw between frames changes nothing.
  Before, every draw of the window also laid out every cell of the strip.
- GPUI gives an `img` the aspect of its image, so a picture in a box of
  another aspect overflowed the box and lost its top and bottom. Each picture
  now has its fitted size, centered in its cell.
- The range is back in frame mode, as whole frames: `frame_range` holds the
  first and the last, and `range` is their time. `[` and `]` take the frame
  on the stage, and a handle snaps to the nearest gap. Delete, duplicate and
  move keep the frames of the range in it. The export keeps the frames in the
  range.
- `s` skips the selected frames. A skipped frame has no time, so it does not
  play or export, and it stays on the strip to play again. One frame always
  plays.
- The frames decode on a background thread and come to the window in
  batches. The edit opens on the first. The export waits for the last.
- `g` shows the grid, with Skip and Copy under each frame. It goes beside the
  stage for a frame narrower than the stage and above it for a wider one, in
  the room the letterbox leaves, between 30% and 60% of it. Only the rows in
  sight draw.

### 2026-10-11, third pass

The fans still ran while a GIF played. The user asked for smaller frames in the
grid, a scrollbar on it, and suggested a video proxy for playback.

Measured in a sim build with the crate at `opt-level 3`, on a 204-frame GIF of
880 x 586 at 40 ms, CPU of the window process as a share of one core:

| State | CPU |
| --- | --- |
| A video in the Studio | 11.5% |
| GIF, first pass, all strip cells | 21-29% |
| GIF, the strip hidden | 3% |
| GIF, cells in sight only | 6-10% |
| GIF, cells in sight and the grid | 6-15%, with bursts |
| GIF paused | 2-4% |

- The strip cost most: every cell was built on every frame. It now builds
  only the cells in sight, each at its place in a row of the full width, and
  scrolls by place.
- A video proxy would not help. The video element draws the whole window on
  every display frame and makes a new `RenderImage` for each frame, as the
  stage of frame mode does. The image work is 11 ms of each second.
- The grid buttons are plain elements now, not kit buttons. The cells are at
  least 88 px wide, and the grid has a scrollbar.
- In the sim window, which sits behind the others and draws at about 28 fps,
  playback by the display frame and by the timer cost the same on average.
  The timer stays, because the user's window draws at the display rate.

Checked in the sim build: the grid beside the stage, its scrollbar, and the
strip after a scroll.

Then a drag in the grid moves the selected frames, as on the strip. It has
its own drag type, because GPUI gives a drag move to every listener of its
type, wherever the pointer is. A skipped frame's Skip button goes dim. The
user checked the range handles, the drag on the strip, Copy, a click on a
skipped frame, the loading, and `[` and `]` on 2026-10-11.
