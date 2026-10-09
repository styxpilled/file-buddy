# 43 - Frame mode for GIF and animated WebP

Status: ready-for-agent

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
