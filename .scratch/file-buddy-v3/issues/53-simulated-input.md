# 53 - Drive the dev window with simulated input

Status: done

Depends on nothing. Read `docs/agents/checking-the-window.md` first.

`drive.ps1` uses real input. It takes the user's screen, so each check needs
their permission. Most checks only need GPUI to see the events and to draw a
frame. The dev build can do that by itself, behind the user's other windows,
with no real input and no question first. `drive.ps1` stays for the final
check of a feature.

## What GPUI has

All in `gpui-pre` 0.3.1:

- `Window::dispatch_keystroke` and `Window::dispatch_event` are public. They
  go through the same dispatch as real input: key bindings, focus, hover and
  drags.
- `Window::render_to_image` draws the current scene to an image without
  presenting it. It is behind the `test-support` feature. The Windows DirectX
  renderer has it (`gpui-pre-windows/src/directx_renderer.rs`).

## Spike first

Answer these before anything else is built, and write the answers here:

1. Does `gpui-pre/test-support` build in this project, in a debug build?
2. Does a window behind other windows, never activated, keep drawing frames?
   A minimized window probably does not.
3. Do video frames, from `gpui-video` through `paint_image`, show in
   `render_to_image`?

If 2 or 3 fails, record why and stop.

### Answers

1. Yes. `gpui-pre/test-support` alone is not sufficient: the Windows
   `render_to_image` is behind the `test-support` feature of
   `gpui-pre-windows`. `gpui-pre-platform/test-support` turns on both, so `sim`
   has a direct optional dependency on `gpui-pre-platform` too.
2. Yes. The window opens with `SW_SHOWNOACTIVATE`, goes to `HWND_BOTTOM`, and
   is never activated. Typed text, clicks, scrolls and drags all show in the
   next picture. GPUI draws an inactive window at 30 frames a second. A
   minimized window was not tested.
3. Yes. Two pictures 1.5 seconds apart during playback show different frames
   in the side pane.

## Work

- A Cargo feature, `sim`, that turns on `gpui-pre/test-support` through a
  direct dependency on `gpui-pre`, at the version `gpui-kit` pins. A release
  build never has it.
- With `sim`, the dev app opens a named pipe. Each line is one step, in the
  `drive.ps1` step language: `keys:`, `kdown:`, `kup:`, `click:`, `dblclick:`,
  `rclick:`, `mclick:`, `down:`, `move:`, `up`, `mdown:`, `mup`, `scroll:`,
  `hover:`, `shot:`, `wait:`. A reply line says what happened: the picture
  path for `shot:`, the cursor for `hover:`, or an error.
- Coordinates are window pixels from the top left, as in `drive.ps1`, so a
  step list works in both scripts.
- `keys:` takes the SendKeys spelling that `drive.ps1` uses, such as `{ESC}`,
  and turns each key into a `Keystroke`.
- `shot:` writes `render_to_image` as a PNG.
- With `sim`, the window opens without focus and does not come to the front.
- `scripts/sim.ps1` takes the same `-Steps` and `-OutDir` as `drive.ps1`,
  sends them down the pipe, and prints the replies.
- In `checking-the-window.md`: use `sim.ps1` for checks during the work, with
  no question to the user. Use `drive.ps1`, after asking, for the final check,
  and for anything `sim` cannot do.

## What sim cannot do

Native Windows UI: the shell's delete dialogs, a drop from Explorer, the title
bar buttons, and a paste into another application. Anything that depends on
the window being the active window. Write each one found here.

Found:

- `space` did not play, pause or hold for 2x. `FileBuddy::render` cancelled a
  press in every frame while the window was not active. The press is there
  for a hold that is still down when the user leaves the window, which gets no
  release. It is now cancelled when the window becomes inactive, which keeps
  that case and lets sim press keys.
- `hover` cannot read the cursor. GPUI keeps the cursor it picks to itself, and
  sets it only while the real pointer is over the window. The reply says
  `unknown`. The pointer still moves, so a picture shows the hover state.

## Comments
