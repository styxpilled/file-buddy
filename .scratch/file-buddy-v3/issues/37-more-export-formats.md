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

AV1 levels were matched to the H.264 levels by SSIM on an 8 s, 480x852 clip
from Downloads, with frames aligned by index: Best 15, High 20, Medium 25,
Low 32. At equal SSIM, `av1_nvenc` wrote larger files than `h264_nvenc`:
3.7 MB against 2.5 MB at Best, 1.9 MB against 1.0 MB at Medium.

The AV1 levels above are replaced. See "Export quality levels" in
`research.md`: two lower levels, Lower and Lowest, were added for all formats,
and the AV1 levels were measured again on four clips.
