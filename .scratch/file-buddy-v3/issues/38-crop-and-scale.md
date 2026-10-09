# 38 - Crop and scale

Status: ready-for-agent

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
