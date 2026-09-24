# 26 - Custom keys

Status: done

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

### Built, 2026-09-24

- `Config::keys` is the `[keys]` table. It is not written when it is empty.
- `ui/keys.rs` holds the actions that can change with their defaults, the
  fixed keys with their actions, and `resolve`, which joins the defaults
  with the changes. The defaults take their keys first, so in a conflict the
  change loses the key. The conflict names both actions. Rows keys from issue
  25 (`delete`, `l`) take the `Rows` context in place of `FileBuddy`.
- A key must be a single character, an F key or a named key. GPUI parses
  any word as a key, and `blah` would never fire. A key sequence is skipped.
- `apply` keeps a copy of gpui-component's keys on the first call, then
  clears the keymap and binds that copy, the fixed keys and the resolved
  keys. It runs on start and on a save that changes the keys.
- An entry that cannot apply is named in the header and under the Keys
  list. A save with a conflict is refused, and the header names it.
- The Keys section of Settings: up or down on the page goes into the list.
  Enter captures the next key, `delete` removes the last key, `r` resets
  the row, and escape cancels a capture. A changed row is marked. The key is
  taken by `intercept_keystrokes`, before the keymap, so it does not also
  fire its action.
- The history keys of issue 31 are held as fixed keys already.
- The wheel over the Keys list scrolls only the list. GPUI scrolls every
  scroll area under the pointer, so a frame around the list stops the event.
  The grid, the table, the Settings page and the Keys list show a scrollbar.
- Unit tests: the defaults, context from the key, conflicts, fixed keys,
  skipped entries, an empty list that unbinds, and the round trip of the
  table through the file.

Checked in the window: an unknown action shows in the header. In Settings,
`q` added to `rename_file` did not fire any action, `x` as well showed the
conflict with `toggle_mark` and the save was refused. After `delete` the
save went through, and `q` on Browse opened the rename input with no
restart.
