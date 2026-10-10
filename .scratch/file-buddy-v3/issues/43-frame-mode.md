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
