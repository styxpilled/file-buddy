# 52 - Paste to add

Status: done

Depends on 45. Read the Paste section of the v3 spec first.

## Work

- `ctrl-v` outside the text inputs reads the clipboard.
- Files as `CF_HDROP`: the same path as a drop.
- An image as the registered `PNG` format, or `CF_DIB`: save it to the export
  folder as `pasted <date> <time>.png`. Select it when the scan finds it. With
  no export folder, the header says to set one.
- `ctrl-v` in a text input still pastes text. It is a fixed key, and the one
  `ctrl` key that works only outside the inputs. Update the key context rule
  and its tests.

## Check

Use a scratch copy, never the real roots. Paste an image copied from a browser,
and files copied in Explorer.

## Comments

Done.

- `clipboard::paste` reads `CF_HDROP` first, then the registered `PNG`
  format, then `CF_DIB`. The application decodes a DIB of 24 or 32 bits,
  uncompressed or with bit fields, top-down or bottom-up. A 32-bit DIB with no
  alpha is opaque. Any other DIB is an error in the header.
- Files go to `add_dropped`, as a drop does.
- An image is written to the export folder as
  `pasted <yyyy-mm-dd> <hh-mm-ss>.png`. The name has a counter if the file is
  there. After each watcher batch, `select_pasted` looks the path up in the
  index and selects the file if the results show it.
- `ctrl-v` is a fixed key, `paste`, bound outside the inputs. In
  `works_in_inputs`, `ctrl-v` is the one `ctrl` key that does not work in an
  input. Its context test has `ctrl-v` and `ctrl-shift-v`.
- Tests: a DIB round trip, a padded top-down 24-bit DIB, a 32-bit DIB with
  zero alpha.

Checked with sim on a scratch copy of the dev build with its own config, root
and database, all removed after:
- An image in the browser form (`PNG` and a bitmap) was saved to the export
  folder and selected.
- A bitmap only was decoded with the right colors and selected. The selection
  moved off the file that had it.
- Two files copied from outside the root joined the Dropped entry, with the
  first selected.
- `ctrl-v` in the search box pasted text, and no file was written.
