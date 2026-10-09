# 37 - WebM, GIF and animated WebP export

Status: ready-for-agent

Depends on 36. Needs 35 so that WebP exports index. Read the Export section
of the v3 spec first.

## Work

- A format picker in the panel: MP4, WebM, GIF, animated WebP.
- WebM is AV1 through `av1_nvenc`, with Opus. There is no VP9.
- GIF goes through `palettegen` and `paletteuse` in one FFmpeg call, with an
  fps control.
- Animated WebP is `libwebp_anim`, with fps and quality controls.
- GIF and WebP have no audio.
- The controls are direct. There is no size target. The header shows the size
  after the export.
- Each format keeps its own controls until the application closes.

## Tests

The arguments for each format.

## Check

Export one range in each format. Each pastes, plays, and indexes from the
export folder. A WebP export probes as animated.

## Comments
