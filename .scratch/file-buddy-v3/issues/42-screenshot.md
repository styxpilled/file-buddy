# 42 - Screenshot

Status: done

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

Ask the user to paste into Discord and Photoshop. The size matches the source, or the crop.

## Comments

### 2026-10-10

Built. The user pasted into Discord and Photoshop on 2026-10-11, and both are correct.

- `p` (`screenshot`) works on every page. In the Studio it takes the edit,
  and elsewhere the selection: a video at the position of the pane clip, or
  at 0 with no clip open, and a still as it is.
- `export::screenshot_args` builds one frame: `-ss` before the input, the
  crop, the texts that show at that moment, no scale, and PNG on
  `image2pipe`. A seek before the input is exact: the integration test
  matches the frame pixel for pixel against a seek after the input.
- The crop has `exact=1`. Without it, FFmpeg rounds an odd crop of a
  yuv420p frame down to even. The text math still uses the even output
  size, so a text on an odd crop can be off by half a pixel.
- `clipboard::copy_image` writes the registered `PNG` format, which keeps
  alpha, and `CF_DIB` as a 32-bit bottom-up BGRA bitmap. Windows makes
  `CF_BITMAP` and `CF_DIBV5` from it. The `png` feature of `image` decodes
  the frame. The crate was already in the tree with it.
- FFmpeg and the clipboard run on the background executor. The header says
  `copied the frame, <w> x <h>`, or the error.
- Checked in the sim build: a still in the pane (850 x 400), a video in the
  pane (1080 x 1920), and a 1:1 crop with a text and a 480 maximum width in
  the Studio (1080 x 1080, the same frame as the preview).
