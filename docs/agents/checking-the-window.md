# Checking the window

## Two scripts

- **`scripts/sim.ps1` for checks during the work.** It needs no question to the
  user. A dev build with the `sim` feature takes the steps through a named pipe
  and sends them through GPUI's own dispatch. The window stays behind the
  user's windows, and no real input moves.
- **`scripts/drive.ps1` for the final check of a feature**, and for anything
  sim cannot do. It uses real input, so ask first.

Both take the same steps at the same coordinates, so one step list works in
both.

    cargo run --bin file-buddy --features sim &
    # ...
    & ./scripts/sim.ps1 -OutDir <scratch> -Steps @('keys:sopr','shot:narrowed','keys:{ESC}','shot:cleared')

### Using sim

1. Stop the dev build first if it runs: `& ./scripts/stop-dev.ps1`. A running
   debug exe makes the build fail with "Access is denied".
2. Build and start it: `cargo build --bin file-buddy --features sim`, then
   `Start-Process target\debug\file-buddy.exe`. Wait about 5 seconds for the
   pipe. The window opens behind the others and does not take focus.
3. Run the steps with `sim.ps1`. It prints the picture path of each `shot`, and
   it stops with the error if a step fails. Read the pictures, then delete them.
4. Stop it with `stop-dev.ps1` when the check is done.

Things to know:

- Each step waits 250 ms for a frame to be drawn, and `shot` waits 250 ms more
  first. `kdown` waits only 30 ms, so a `kdown` and `kup` pair is a tap. Use
  `wait:` for anything slower, such as a clip that opens after the selection
  stops moving (about 1.5 s).
- `keys:` sends each key down and up, as SendKeys does. `keys: ` types a space.
  A modifier held with `kdown:16`, `17` or `18` applies to the keys and
  clicks that follow it, until its `kup`.
- The audio starts muted in a `sim` build, so a check never plays sound.
- Take a fresh `shot` before you click. The coordinates are those of the last
  picture.
- To see why a key did nothing, start the exe with `RUST_LOG=file_buddy=warn`
  and `-RedirectStandardError <file>`, and add a temporary `log::warn!`.
- To compare one region over several pictures, crop it with
  `System.Drawing` into one strip and read that. This uses fewer tokens than
  full pictures.

### What sim cannot do

- Native Windows UI: the shell's delete dialogs, a drop from Explorer, the
  title bar buttons, and a paste into another application.
- Anything that depends on the active window. The window never becomes active.
- `hover` cannot read the cursor. It moves the pointer, so a picture shows the
  hover state, but the reply says `unknown`.
- A minimized window. Do not minimize the dev window.

## Rules for drive.ps1

- **Ask before you take the screen.** `drive.ps1` moves the mouse, types and
  covers the desktop. Ask the user first. One answer covers the checks that
  follow it closely in the same session. After a break, ask again.
- **The user's own build runs at the same time.** It is a release build in
  `C:\Users\styx\bin\file-buddy`, with the window title `file-buddy`. Both
  builds have the process names `file-buddy` and `fb`, so never stop or pick a
  process by name.
- **Drive only the dev window.** A debug build has the title `file-buddy dev`.
  `drive.ps1` picks the window by that exact title, and it refuses to run if
  that window is not open.
- **Stop only the dev build.** `scripts/stop-dev.ps1` stops the processes whose
  executable is in `target\debug`, and nothing else.

## drive.ps1

`scripts/drive.ps1` types into the running application and saves a picture of
its window. Use it when a change only shows itself in the real window. Tests
cannot see focus, key bindings or layout.

    cargo run --bin file-buddy &
    # ...
    & ./scripts/stop-dev.ps1
    & ./scripts/drive.ps1 -OutDir <scratch> -Steps @('keys:sopr','shot:narrowed','keys:{ESC}','shot:cleared')

Call it from PowerShell as above. `pwsh -File` gives the whole list to `-Steps`
as one string.

It prints the path of each picture. Steps run in order: `keys:<SendKeys>`,
`click:<x>,<y>`, `dblclick:<x>,<y>`, `down:<x>,<y>`, `move:<x>,<y>`, `up`,
`mclick:<x>,<y>`, `rclick:<x>,<y>`, `mdown:<x>,<y>`, `mup`, `kdown:<vk>`, `kup:<vk>`,
`scroll:<x>,<y>,<notches>`,
`hover:<x>,<y>`, `shot:<name>`, `wait:<milliseconds>`. `hover` prints the cursor
the window shows at that point, `hand`, `arrow`, `updown` or `other`, because a picture
does not hold the cursor. `down`, `move` and `up` make a drag, and a `shot`
between them shows it in progress. Each step waits a moment after it, so two `click` steps
are never a double click. `SendKeys` spells the special keys `{ESC}`, `{DOWN}`,
`{ENTER}`, `{TAB}`. A click is measured from the
top left corner of the window, which is where a picture of the window measures
from too, so a button can be clicked at the coordinates it has in the last
picture. Take a fresh picture first: the window may have moved or changed size.

## What it does, and why it does it that way

**Windows refuses `SetForegroundWindow`** from a process that is not already in
front. It returns `False` and raises nothing, so the keystrokes go to whatever
window is in front, which may be anything the user has open.
`AttachThreadInput` to the thread that owns the current foreground window lifts
that block. This is the only reason the script exists.

**The terminal goes full screen first**, so nothing of the user's desktop is
under or beside the application window. The first version restored the terminal
instead of maximizing it, which shrank it and uncovered everything. Only a
minimized window is restored now. A terminal that was not maximized goes back to
its size at the end.

**The picture is the application window only**, from `GetWindowRect`, never the
whole screen.

**Every step runs in one call.** A new process starts behind the terminal, so
activation, keystrokes and pictures that are split across calls do not see the
same foreground window.

Delete the pictures when you have read them.
