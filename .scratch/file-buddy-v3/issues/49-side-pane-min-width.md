# 49 - Side pane minimum width

Status: ready-for-agent

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
