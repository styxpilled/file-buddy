# 46 - Context menu

Status: ready-for-agent

Depends on 36 for Open in Studio and 45 for Delete from database. Read the
Context menu section of the v3 spec first.

## Work

- A right click on a tile or a table row opens a menu of file actions.
- On a marked file, it acts on every marked file. Otherwise the click selects
  the file first.
- Items: Copy, Reveal, Rename, Open, Open in Studio, More like this, Tag, then
  Trash, Delete and Delete from database. Each shows its key.
- An item that needs one file is disabled for several. Delete from database is
  disabled when no file is dropped.
- `file_menu` in `src/ui/preview.rs` builds the side pane menu. Move the list
  out, so one list builds both menus. The side pane menu gains the new items.
- Keep the `menu_open` handling, or the window takes focus back and closes the
  menu.

## Comments
