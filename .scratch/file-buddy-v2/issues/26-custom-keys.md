# 26 - Custom keys

Status: ready-for-agent

Depends on nothing in v2. Read the Custom keys section of the v2 spec first.
New actions from 18 to 25, such as `a` for more like this, join the list the
same way.

## Work

- Add a `[keys]` table to `Config`: action name in snake case, then a list of
  keys. The names come from the `actions!` list in `src/ui/mod.rs`, for example
  `rename_file` for `RenameFile`.
- Replace the fixed list in `bind_keys` with the defaults plus the overrides.
  The context comes from the key: `FileBuddy` for a key with `ctrl` or `alt`, or
  an F key, and `FileBuddy && !Input` for any other key.
- Keep the fixed keys out of `[keys]`: arrows, `enter`, `escape`, `backspace`,
  `ctrl-c`, `ctrl-s`, `ctrl-1` to `ctrl-4`, `alt-up`, `alt-down`, and the
  history keys from 31: `alt-left`, `alt-right` and the mouse back and forward
  buttons. Their actions
  do not show in the table or in Settings.
- Refuse a save with a conflict: two actions on one key, or a fixed key. Name
  both actions.
- Skip an unknown action or a key GPUI cannot parse. Apply the rest. Show what
  was skipped in the header and in the Keys section.
- Apply on start and on save. `cx.clear_key_bindings()` removes
  gpui-component's keys too, so bind them again before the application's keys.
- Settings gets a Keys section: Enter captures a key and adds it, `delete`
  removes the last key of the row, `r` resets the row, escape cancels a
  capture. Mark rows that differ from the default. The capture must not fire
  the action it captures.
- Unit tests: context from the key, conflicts, fixed keys, skipped entries, and
  an empty list that unbinds.

## Comments
