# 44 - Image sequence

Status: done

Depends on 43. Read the Image sequence section of the v3 spec first.

## Work

- With two or more stills marked, `c` opens them as a sequence, in name order,
  in frame mode.
- The canvas size starts at the first image's size. The panel changes it.
- Fit or fill for every frame. Fit has transparent bars, black in MP4 and
  WebM. Fill covers the canvas, centered.
- Each frame then has its own offset and scale. A drag on the preview moves
  the selected frame, the wheel scales it, and Reset puts it back.
- While a frame is selected, the frame before it shows under it at half
  opacity.
- Each frame starts at 100 ms.
- The export name is `<first stem> sequence.<ext>`.

## Tests

Fit and fill with an offset and a scale, for wide and tall images on a canvas.

## Comments

### 2026-10-11

Built. Checked in the sim build on three stills of different shapes. Not yet
checked by the user.

- With two or more files marked, `c` opens them as a sequence if all are
  stills, in name order, case-insensitive. If any marked file is not a
  still, the header says "a sequence takes only stills". With fewer than two
  marks, `c` opens the selection as before.
- FFmpeg decodes each still to PNG, as the still export reads it, so every
  still kind opens (BMP and AVIF too). A still that fails is left out and the
  header says some frames cannot open.
- The frames keep their own images. `Frame` has a `place`: an offset as a
  share of the canvas and a scale. The stage, the strip and the grid place the
  image in GPUI. The export composes each frame on a transparent canvas with
  Lanczos, and scales only the part on the canvas, so a large scale costs no
  more than the canvas.
- A drag inside the crop box moves the selected frames, or the frame on the
  stage with none selected. So in a sequence the crop box moves only by its
  grips. The wheel over the stage scales them, 1.1x per notch, from 0.05x to
  20x. Reset puts them back.
- The canvas is two inputs, from 16 to 8192 px. A new size resets the crop.
- The frame before shows over the selected frame at half opacity, not under
  it: an opaque frame on top would hide a frame under it. Where both are
  opaque the result is the same 50/50 blend. It is the frame before that
  plays, so a skipped frame is passed over.
- GIF and WebP keep the bars transparent. MP4 and WebM make them black.
- `image-webp` does not clear a frame that disposes of itself when no
  background color is set, so the next frame showed over the last one. Both
  WebP decoders now set a transparent background, as a browser shows it. This
  also fixes frame mode and the proxy for such WebP files.
- Tests: fit and fill with an offset and a scale for wide and tall images,
  the composed pixels, a large scale, the export name, and an export of a
  sequence as GIF, WebP and MP4 that checks the bars.

### 2026-10-11, second pass

The user asked for six changes.

- A moved frame snaps to the canvas: an image edge within 8 screen pixels of
  a canvas edge goes onto it.
- A double click on the name in the panel opens an input for the name of the
  export. It starts with the name the export would take, with no extension.
  Enter or a click elsewhere keeps it, and escape drops it. Characters Windows
  refuses go. An empty name, or the name the export would take anyway, gives
  the default back. A name that exists still gets ` (2)`. This works for
  every source, not only a sequence.
- The hint under the canvas is gone. Reset now sits in the canvas row. A
  context menu on the frames is noted in issue 46.
- The frame count, the range of frames and the selection are one row:
  `1-3 / 12, 2 selected`, then the length and Grid. The count of skipped
  frames does not show.
- The grid says "skip" for a skipped frame, as the strip does.
- In frame mode, a drag on an end of a text's bar steps from gap to gap
  between frames and keeps one frame, as the range handles do. A drag on the
  middle of the bar starts the text at the nearest gap too, so the text stays
  on whole frames.

Checked in the sim build: the snap, the rename and the panel row.

### 2026-10-11, third pass

- A GIF or an animated WebP now has the canvas, Fit and Fill, the place of
  each frame and the frame before at half opacity, as a sequence has. Its
  canvas starts at its own size, so nothing changes until a frame moves. A
  frame that still fills the canvas goes to the export with no work. The
  `sequence` flag now only names the export.
- Ctrl and the wheel scale the frames in frame mode. The wheel alone sets the
  sound level, as for a video. On a video, ctrl and the wheel set the sound
  level too.
- Ctrl and the wheel over a text change its size, from 1% to 100% of the
  frame height, and select it. Each wheel line is 4%, and Windows sends three
  lines a notch.
- The size presets are gone. Each text row in the panel has its size in px,
  Box and Delete.
- The Edit and Text tabs are gone. The texts are at the bottom of the panel,
  under Export, and their list scrolls.

Checked in the sim build on a GIF: ctrl and the wheel on a frame, a drag, and
ctrl and the wheel on a text.

### 2026-10-11, fourth pass

- A drag on the stage does not move a frame while the frames play, and the
  frames do not play during a drag.
- The frame before the selected one shows at 30% opacity, not 50%, so the
  selected frame is clear.
- The text list has no hint when it is empty. Its button is "(N)ew".
- The texts are under Speed and over Format. Export is at the right.

The user accepted the issue on 2026-10-11.
