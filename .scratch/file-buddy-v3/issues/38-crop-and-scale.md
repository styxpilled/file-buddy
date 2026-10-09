# 38 - Crop and scale

Status: done

Depends on 36. Read the Crop and Scale sections of the v3 spec first.

## Work

- A crop box on the preview. Outside it, the preview is dark. Drag to move,
  drag a corner or an edge to resize.
- Aspect: free, source, 1:1, 16:9, 9:16.
- Maximum width: source, 1080, 720, 480, 360, or a number. The output never
  grows larger than the crop.
- Output dimensions are even.
- Add `crop` and `scale` to the argument builder, after the trim.

## Tests

Crop and scale arguments, the aspect lock, even dimensions, and no growth past
the crop.

## Comments

### 2026-10-09

Done. The crop is in source pixels, and the frame size comes from the decoder.
A press on the box that moves becomes a drag, so it does not pause the clip;
a press that does not move is still a tap or a hold. The panel shows the
output size. A custom maximum width goes in the input after the presets.

NVENC refuses small frames: `h264_nvenc` needs 145 x 49 or more, and
`av1_nvenc` 129 x 65 or more. MP4 and WebM exports below 146 x 50 and
130 x 66 stop with a message in the header. GIF and WebP have no minimum.
