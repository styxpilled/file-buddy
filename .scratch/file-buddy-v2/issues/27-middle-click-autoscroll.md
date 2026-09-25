# 27 - Middle-click autoscroll

Status: done

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

### Spike answers (2026-09-25)

GPUI can do it. Built in `src/ui/autoscroll.rs`.

- The virtual list handle derefs to the GPUI `ScrollHandle`, so `offset()`,
  `set_offset()` and `max_offset()` are there. The list clamps the offset
  again when it lays out.
- The cursor is `ResizeUpDown`, which is `IDC_SIZENS` on Windows. There is no
  all-scroll cursor in `CursorStyle`.
- Mouse moves outside the window: no, not after a click. GPUI calls
  `SetCapture` only between a button press and its release. The list keeps the
  speed it had when the pointer left, and a click or escape still ends it. A
  drag with the wheel held keeps the capture, so the moves arrive from
  anywhere on the screen.
- The cursor in a drag: Windows sends no `WM_SETCURSOR` while the window holds
  the capture, and GPUI sets the cursor only from that message. So the start
  also calls `SetCursor` directly.
- What drives the scroll: `Window::request_animation_frame` from `render`. The
  step runs before the list lays out, so there is no frame of lag. The step is
  the time since the last frame, capped at 50 ms. In the dead zone no frames
  are asked for.
- Hover video: yes, it would start. GPUI tests hover against the hitboxes on
  each frame, not only on a mouse move, so a tile that moves under a still
  pointer gets `on_hover(true)`. `set_hovered` now waits while autoscroll is
  on, and the tile under the pointer plays when it ends.
- Middle click today: nothing. `on_click` takes the left button only, and the
  app has no `on_aux_click`.

Other results:

- Speed is 10 px/s for each pixel past a 12 px dead zone.
- The window listeners are in the capture phase and stop propagation, so the
  click that ends autoscroll does not also select a tile, and the wheel turn
  that ends it does not scroll.
- It also ends when the window loses focus, when the results are empty and on
  Settings.
- `scripts/drive.ps1` has `mclick`, `mdown` and `mup` now, and `hover` can
  report `updown`.

Checked in the window: click mode, drag mode, escape, a left click, a wheel
turn, the table, and the cursor in both modes. In the first run only, the list
stopped after about one second. The user may have touched the mouse or keys
during that run, and five later runs with a log of each end did not show it.
