# 39 - Speed

Status: ready-for-agent

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
