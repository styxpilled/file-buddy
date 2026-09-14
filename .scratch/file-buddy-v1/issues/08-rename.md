# 08 - Rename and the bulk walk

Status: ready-for-agent

Depends on 04. The filename is the highest weighted machine-readable signal, so
this is what turns unfindable files into findable ones.

## Work

- Inline rename on the selected file, prefilled with the current name.
- Renaming writes to the real file on disk. Sanitize characters that Windows
  forbids. Refuse a name that already exists. Never overwrite.
- A rename updates the filename column only. Identity is the content hash, so
  nothing is transcribed again.
- Bulk walk: step through files with no proper name, one at a time. The clip
  plays in the pane, the user types a name, Enter saves and moves to the next.

## Not in scope

No suggested names. The transcript is a search signal only. The application does
not try to be clever about what a file should be called.
