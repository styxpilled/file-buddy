# 05 - File actions

Status: done

Depends on 04. After this issue the application is useful every day.

## Work

- Enter copies the selected file with `clipboard-win`. Write two formats at
  once: `CF_HDROP` so an application such as Discord uploads the file, and
  `CF_UNICODETEXT` with the path so a text field gets something useful. GPUI
  cannot do this. Its `ClipboardEntry` holds only a string or an image.
- Reveal in Explorer with `App::reveal_path`. Built into GPUI.
- Open with the default application with `App::open_with_system`. Built in.
- Show a short confirmation when a copy succeeds.

## Out of scope

Drag out of the window. GPUI has no drag source at all: `on_drag` works only
inside the window, and there is no `DoDragDrop` or `IDropSource` anywhere in the
crate. Revisit only if the clipboard feels bad in real use.

## Comments

Done.

- `src/clipboard.rs` writes `CF_UNICODETEXT` and `CF_HDROP` in one open.
  `clipboard-win`'s string writer empties the clipboard first, so the module
  empties it once itself and both writes use the `NoClear` form. Order would
  otherwise decide which format survived.
- The roots come from a TOML file, so a path can arrive with forward slashes.
  Explorer pastes that as a file it cannot find, so `copy_file` normalizes the
  separator before writing either format.
- Enter copies. `ctrl-c` does the same, for the hand that expects it. `ctrl-e`
  reveals through `App::reveal_path`, `ctrl-o` opens through
  `App::open_with_system`. The pane has a Copy, Reveal and Open row with those
  keys in the tooltips.
- A confirmation sits in the header for 2.2 s, green for a copy and red for a
  failure, then the result count comes back. It is capped and truncated: a long
  file name was squeezing the search box.
- Each action checks the file is still there first. The index is a snapshot, and
  all three platform calls fail silently on a missing file. A gone file says so.

The round-trip test reads both formats back off the real clipboard, which is the
only way to catch a copy that writes one format and looks fine until it is
pasted somewhere that wants the other. It replaces the clipboard while it runs.

All three actions were checked in the running window: Enter put the file on the
clipboard in both formats, `ctrl-e` opened Explorer on the containing folder,
`ctrl-o` opened the clip in VLC.
