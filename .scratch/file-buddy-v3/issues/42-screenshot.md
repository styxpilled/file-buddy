# 42 - Screenshot

Status: ready-for-agent

Depends on nothing for the side pane. The Studio part needs 38 and 40. Read
the Screenshot section of the v3 spec first.

## Work

- `p` copies the current frame at full size to the clipboard as an image. No
  file is written.
- In the side pane, the raw source frame at its own resolution.
- In the Studio, the frame with crop and text, at the resolution of the crop,
  with no scale. Use the argument builder for one frame.
- FFmpeg extracts the frame at the exact time as PNG on a pipe.
- The clipboard gets `CF_DIB` and the registered `PNG` format, through
  `actions/clipboard.rs`.
- The header confirms the copy.

## Check

Paste into Discord and into Paint. The size matches the source, or the crop.

## Comments
