# 44 - Image sequence

Status: ready-for-agent

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
