# 27 - Middle-click autoscroll

Status: ready-for-agent

Depends on nothing. Applies to the grid and the table on every page.

The user clicks the wheel, then moves the mouse to scroll, as in a browser on
Windows. First find out if GPUI can do it. Then build it if it can.

## Behaviour

- A middle click on the grid or the table starts autoscroll. The point of the
  click is the anchor.
- The list scrolls up or down while the pointer is away from the anchor. The
  speed grows with the distance. A small dead zone around the anchor stops it.
- Any click, a wheel turn or escape ends it.
- Holding the middle button and dragging does the same, and release ends it.
- The cursor shows that autoscroll is on.

## Found without testing

- GPUI has `MouseButton::Middle` (`gpui-pre` 0.3.1, `src/interactive.rs`).
- The grid and the table are `v_virtual_list` from gpui-component, with a
  `VirtualListScrollHandle` in `FileBuddy` (`src/ui/mod.rs`). The GPUI
  `ScrollHandle` has `offset()` and `set_offset()` (`src/elements/div.rs`).
  Check that the virtual list handle gives the same.
- `CursorStyle` has `ResizeUpDown`. No all-scroll cursor was seen. Check the
  full list.

## Questions for the spike

- Does the window get mouse moves when the pointer leaves it after a click with
  no button held? If not, autoscroll stops at the window edge, which may be
  good enough.
- What drives the scroll between mouse moves: `request_animation_frame` or a
  timer on the foreground executor?
- Does the hover video in a tile start while the list scrolls under a pointer
  that does not move? It must not.
- Does a middle click do anything today that this would take away?

Record the answers here. If GPUI cannot do it, say why and stop.

## Comments
