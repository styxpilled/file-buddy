# 31 - Back and forward history

Status: done

Depends on nothing. Read the History section of the v2 spec first. If 22 is
done, "more like this" is a step too. If not, add it in 22.

The back and forward buttons of a mouse, and `alt-left` and `alt-right`, move
through a history of where the user has been. There is no history yet.

## Found without testing

GPUI gives `MouseButton::Navigate(NavigationDirection::Back | Forward)`
(`gpui-pre` 0.3.1, `src/interactive.rs`), and the Windows backend maps
XBUTTON1 and XBUTTON2 to it.

## Work

- A step saves the state the user leaves. These make a step:
  - a page switch, by key, by tab, or by escape back to Browse,
  - opening a folder, Backspace, a breadcrumb click,
  - a Review group,
  - "more like this".
- Typing makes no step. The query is part of the saved state, so "type `cat`,
  open a folder, press back" returns to Browse with `cat` typed.
- A step brings back the page, the folder, the query, the Review group, the
  selection and the scroll position. Follow the selection by file, not by row,
  as `update_results` does.
- The chips, the sort and the view are not part of a step.
- A new step after going back drops the forward steps. History lives in memory,
  is lost on close, and holds at most 100 steps. A folder that no longer exists
  is skipped.
- Back and forward close an open tag panel or rename input and navigate at once,
  as a page switch does today. Unsaved Settings changes behave as on a page
  switch.
- The mouse buttons and `alt-left` and `alt-right` are fixed keys. Add them to
  the fixed list in issue 26. By the rule there, `alt-left` and `alt-right`
  work everywhere, the search box included. Check that gpui-component's text
  input binds neither.
- Unit tests: step, back, forward, the dropped forward steps, the limit, and a
  missing folder.

## Comments

### Built (2026-09-26)

`src/ui/history.rs` holds the steps. A step saves the page, the folder, the
query, the searches by look, the Review group, the selected entry and the scroll
offset. The results come back from a new search, not from memory, so 100 steps
of Browse with 3,416 files hold no results.

- A step restored into "more like this" keeps the query of each search by look,
  but not the results it replaced. Escape there runs the search from before
  again, so its selection starts at the top.
- If the selected file of a step is a visual match, it is not in the text
  matches that show first. The selection moves to it when the visual matches
  merge in.
- gpui-component binds `alt-left` and `alt-right` in the text input only on
  macOS, so on Windows they are free everywhere.

Checked in the window with `alt-left` and `alt-right`: Browse to Folders, a
folder, a query typed there, Backspace, then back three times and forward
twice. Each step brought back its page, folder, query, selection and scroll.
Also checked: back from inside the search box, back from a Review group to the
first group, and back and forward over "more like this". The mouse buttons are
not checked, because `scripts/drive.ps1` cannot press them.

### Mouse buttons over the header (2026-09-26)

The mouse buttons worked only over the page, not over the header. The header
parts that take a click use `occlude()`, which also hides them from the root
element, so a listener on the root never fired. The listener is now a
window-level `on_mouse_event`, as autoscroll uses. The empty header space is
still the title bar to Windows, which sends `WM_NCXBUTTONDOWN` there, and
gpui-windows does not pass that on. Not fixed.
