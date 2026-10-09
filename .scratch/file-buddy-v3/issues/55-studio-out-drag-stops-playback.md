# 55 - A drag of the out handle stops playback in the Studio

Status: done

Found after 36. Test with `monkey opening portal.gif`. It may depend on the
length of the source.

## What happens

1. Open a GIF or a video in the Studio. It plays and loops over the range.
2. Drag the out handle.
3. After the release, the clip does not play again, although it played before
   the drag.
4. Space then starts it at the start of the source, not at the in point.

## Where to look

- A drag on a handle pauses the clip and keeps whether it was paused
  (`SeekBar::hold`), and the release seeks and restores it
  (`SeekBar::let_go`). Check that the release always arrives: the handle takes
  the mouse down, and the release comes through `on_mouse_up` or
  `on_mouse_up_out` on the bar. If it does not arrive, the clip stays paused
  while `Studio::paused` says it plays.
- A short source decodes to its end at once, so the decoder can be at its end
  during the drag. `Video::set_paused(false)` on a video at its end seeks to
  0, which matches step 4. `toggle_studio_paused` seeks to the in point after
  that only when `ended()` or the position is past the out point.
- The keyframe seeks during the drag can leave `seeking` true, and
  `step_studio` does not loop while it is true.

## Check

With `monkey opening portal.gif` and a longer video: drag the out handle while
the clip plays. After the release it plays on from the out point's seek and
loops over the range. Space pauses it, and space again plays it from where it
paused, or from the in point when it is at the out point.

## Comments

### 2026-10-09

Fixed by the hold rework in 54: a drag holds the clip paused, and the release
seeks, then applies the user's choice through `sync_studio_paused`, which
loops back to the in point at the out point. Checked in the sim build with
`monkey opening portal.gif` and a 60 s clip: after a drag of the out handle
the clip plays the range and loops; space pauses it, and space again plays on
from there.
