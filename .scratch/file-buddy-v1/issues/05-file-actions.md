# 05 - File actions

Status: ready-for-agent

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
