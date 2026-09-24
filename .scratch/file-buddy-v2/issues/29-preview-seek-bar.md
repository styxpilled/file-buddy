# 29 - Preview seek bar, skip and click to pause

Status: ready-for-agent

Depends on nothing. Read the Preview controls section of the v2 spec first.
Only the side pane. Tile videos do not change.

## Work

- A thin progress bar under the clip, with `0:12 / 0:45` beside it.
  - A click on the bar seeks.
  - A drag scrubs. The frame follows the pointer, and the seeks are throttled.
  - Hovering over the bar shows the time under the pointer.
- `shift-left` and `shift-right` skip back and forward 5 s. A skip stops at the
  start and at the end of the clip. Both keys can be rebound through issue 26.
- A click on the clip toggles pause. Issue 30 changes this to act on release.

## Seek precision

`Video::seek` in `gpui-video/src/video.rs` ignores its `accurate` flag and lands
on a keyframe, which can be a second or two from the target.

- Make an accurate seek: seek to the keyframe before the target, then decode
  and drop frames up to it.
- Measure how long an accurate seek takes over the clips in `test_videos`,
  including the 1080x2340 ones.
- If it is fast enough to feel instant, use it for a skip, a click on the bar and
  the end of a drag. During a drag, use keyframe seeks.
- If it is not, keep keyframe seeks and record the numbers here.

Issue 23 seeks to a match time and uses the same seek.

## Comments
