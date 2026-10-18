# 49 - Side pane minimum width

Status: done

Depends on nothing. Read the Side pane width section of the v3 spec first.

The side pane's `size_range` starts at 260 px (`src/ui/mod.rs`). The top row,
`controls` in `src/ui/preview.rs`, does not fit in that: Autoplay, Play, Mute,
the speed, the sound level and the file menu.

## Work

Measure the row at its normal size in the running window, with the pane
padding. Set the minimum to that. Do not guess. If the width can come from the
constants in `preview.rs`, use them, so the minimum follows a change to the
row.

## Check

Drag the pane to its minimum. Nothing wraps or clips. Record the width here.

## Comments

Measured in the window at 390 px: Autoplay 74, Play 76, Mute 70, speed 32,
volume 36, menu 24, gaps 8, padding 12, left border 1. The minimum is 385 px,
as `MIN_PANE_WIDTH` in `preview.rs`, the sum of those constants. Autoplay,
speed and the menu now have fixed widths at their measured size, so the sum
holds. Checked with sim: a drag far to the right stops at 385 px, and the row
fits exactly, with nothing wrapped or clipped.
