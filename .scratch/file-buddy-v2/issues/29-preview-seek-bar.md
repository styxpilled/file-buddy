# 29 - Preview seek bar, skip and click to pause

Status: done

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

### Built, 2026-09-24

- `Video::seek(at, true)` seeks to the keyframe before `at`, then decodes and
  drops the frames before it without scaling them. The last frame before the
  target is held until the next frame shows where it ends, because frame
  times are not always evenly spaced: `dale cooper` has a 33 ms gap in a 60
  fps clip. The sound before the target is dropped too, to the sample.
- The old seek set the clock to the target but played from the keyframe, so
  the sound ran ahead of the picture until it caught up. An accurate seek has
  no such gap.
- A paused clip shows the frame of a seek at once. `Video::seeking()` holds
  until then, so the element keeps asking for frames.
- The bar is the unstyled slider of `gpui-base`: a drag seeks to keyframes at
  most every 60 ms, and the release seeks accurately. A click is a press and a
  release, so it ends accurate too.
- `scripts/drive.ps1` has `down`, `move` and `up` steps for a drag.

### Seek times, 2026-09-24

`cargo test --test playback seek_times -- --ignored --nocapture` in
`gpui-video`. Mean and worst of three seeks, to 25 %, 50 % and 90 % of the
clip, until the frame is on screen. A debug build, with the dependencies
optimized as always.

| Clip | Keyframe, paused | Accurate, paused | Accurate, playing |
| --- | --- | --- | --- |
| 828x706, 4 s | 14 / 16 ms | 44 / 60 ms | 50 / 70 ms |
| 1180x682, 16 s | 18 / 27 ms | 84 / 106 ms | 95 / 116 ms |
| 1920x1080, 26 s | 17 / 20 ms | 77 / 98 ms | 82 / 94 ms |
| 720x1280, 11 s | 21 / 28 ms | 99 / 124 ms | 109 / 144 ms |
| 576x1024, 19 s (.mov) | 61 / 66 ms | 57 / 95 ms | 61 / 103 ms |
| 1080x1920, 60 s | 16 / 20 ms | 197 / 303 ms | 221 / 341 ms |
| 1080x2340, 9 s | 29 / 33 ms | 147 / 166 ms | 163 / 178 ms |
| 17 other clips | 8 to 26 ms | 16 to 62 ms | 18 to 83 ms |

The time grows with the frame size and the distance to the keyframe before
the target. The 1080x1920 clip has one keyframe gap of 9.9 s, and 2.8 s on
average. Most seeks are about 100 ms or less, which feels instant, so a skip,
a click and the end of a drag are accurate. The drag itself uses keyframes.

Checked in the window: `shift-right` twice on a paused clip shows 0:10, the
pointer over the bar shows its time, a click on the bar seeks there, a click
on the clip plays it, `shift-left` goes back 5 s, and a drag moves the frame
with the pointer and ends on the frame where it was released.
