# Checking the window

`scripts/drive.ps1` types into the running application and saves a picture of
its window. Use it when a change only shows itself in the real window. Tests
cannot see focus, key bindings or layout.

    cargo run --bin file-buddy &
    & ./scripts/drive.ps1 -OutDir <scratch> -Steps @('keys:sopr','shot:narrowed','keys:{ESC}','shot:cleared')

Call it from PowerShell as above. `pwsh -File` gives the whole list to `-Steps`
as one string.

It prints the path of each picture. Steps run in order: `keys:<SendKeys>`,
`click:<x>,<y>`, `dblclick:<x>,<y>`, `down:<x>,<y>`, `move:<x>,<y>`, `up`,
`mclick:<x>,<y>`, `mdown:<x>,<y>`, `mup`, `kdown:<vk>`, `kup:<vk>`,
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
