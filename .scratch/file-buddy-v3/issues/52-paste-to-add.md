# 52 - Paste to add

Status: ready-for-agent

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
