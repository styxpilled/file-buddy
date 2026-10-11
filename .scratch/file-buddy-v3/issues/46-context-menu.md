# 46 - Context menu

Status: done

Depends on 36 for Open in Studio and 45 for Forget. Read the
Context menu section of the v3 spec first.

## Work

- A right click on a tile or a table row opens a menu of file actions.
- On a marked file, it acts on every marked file. Otherwise the click selects
  the file first.
- Items: Copy, Reveal, Rename, Open, Open in Studio, More like this, Tag, then
  Trash, Delete and Forget. Each shows its key.
- An item that needs one file is disabled for several.
- `file_menu` in `src/ui/preview.rs` builds the side pane menu. It has Forget
  since 45. Move the list out, so one list builds both menus. The side pane
  menu gains the new items.
- Keep the `menu_open` handling, or the window takes focus back and closes the
  menu.

## Comments

### 2026-10-11

From the work on 44. The frames on the strip and in the grid of the Studio need
a context menu of their own. It holds Reset, which puts the selected frames of
an image sequence back to the fit or fill, in place of the Reset button in the
panel. It can take more later, such as Skip, Duplicate, Delete and Copy.

### 2026-10-11

Done, with the frame menu from the comment above.

- `src/ui/menu.rs` holds the list of file actions. The side pane's file menu
  and the menu of a tile or a table row use it. The Studio's file menu keeps
  Copy, Reveal and Open.
- A right click on an unmarked file clears the marks and selects the file, as
  Explorer does. On a marked file the marks stay.
- The window takes focus back on every frame, so a context menu sets
  `menu_open` when it opens and clears it on its `DismissEvent`.
- The frame menu has Copy, Skip, Duplicate, Reset and Delete. Reset is the new
  `reset_frames` action, with no default key. The Reset button of the canvas
  row is gone.

Checked in sim: the menu on a tile and on a table row, the keys beside the
items, the one-file items disabled with two files marked, Escape, the arrow
keys after the menu closes, Tag from the menu, and Skip from the frame menu on
a test GIF, deleted after.

### 2026-10-11, later

- The grid of frames lost its Skip and Copy buttons. The frame menu has both.
- The menu of a right click fades in over 120 ms. The window draws the
  `PopupMenu` itself in `menu::open_menu`, in place of the kit's
  `ContextMenu`, which has no way to animate it. `context_menu` holds the open
  menu, and `hold_focus` waits while it is open.
- Any key but the arrow keys, enter and escape closes the menu, and then acts
  as it would with no menu: `menu::close_on_keys` intercepts the keystroke and
  gives the focus back before the keymap runs. The menu now takes focus after
  the mouse down ends. Before, the window kept it, so the arrow keys moved the
  selection under the menu.
- A right click on a frame selects it but does not move the stage to it.
- The mouse back and forward buttons close the menu. Their handler stops the
  event in the capture phase, so the menu never saw it.
