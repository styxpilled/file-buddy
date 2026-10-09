# 39 - Speed

Status: done

Depends on 36. Read the Speed section of the v3 spec first.

## Work

- Speed presets from 0.5x to 4x in the panel.
- The preview plays at the speed. `gpui-video` already plays at 2x for issue
  30, so reuse that rate control.
- `setpts` and `atempo` go last in the argument builder. `atempo` keeps the
  pitch. Chain it if one filter cannot reach the speed.
- The range length in the panel is the length after the speed change.

## Tests

The arguments at 0.5x, 1x and 4x, with and without audio.

## Comments

### 2026-10-09

Done. The presets are 0.5x, 0.75x, 1x, 1.5x, 2x, 3x and 4x. The speed button
and a hold of the preview multiply the speed of the edit, so 4x with the button
on plays at 8x. The range points stay in source time, and the export name uses
them.

`atempo` takes steps of 2x, because one filter above 2x skips samples. 3x is
`atempo=2,atempo=1.5`. A source with no sound takes `-af` with no error. The
frame rate of a GIF or a WebP applies after `setpts`. An MP4 at 4x keeps the
source rate and drops frames; at 0.5x it plays at half the source rate.
