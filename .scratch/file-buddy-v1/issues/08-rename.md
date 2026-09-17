# 08 - Rename and the bulk walk

Status: done

Depends on 04. The filename is the highest weighted machine-readable signal, so
this is what turns unfindable files into findable ones.

## Work

- Inline rename on the selected file, prefilled with the current name. The extension should be excluded from the rename.
- The shortcuts for renaming should be: r, ctrl+r, and F2.
- Renaming writes to the real file on disk. Sanitize characters that Windows
  forbids. Refuse a name that already exists. Never overwrite.
- A rename updates the filename column only. Identity is the content hash, so
  nothing is transcribed again.
- Bulk walk: step through files with no proper name, one at a time. The clip
  plays in the pane, the user types a name, Enter saves and moves to the next.

## Not in scope

No suggested names. The transcript is a search signal only. The application does
not try to be clever about what a file should be called.

## Comments

Done.

### Poor names

A stem is poor when it holds `_`, `-`, a bracket, a character outside ASCII, or a
run of more than 4 digits. `media::filename::is_poor` has the rule and its
tests. `db::files::poorly_named` applies it in Rust, because SQL cannot say it
simply, and orders the files by path so one folder comes at a time.

### The rename

- `actions::rename` does the work without GPUI. The typed text replaces the
  stem and the extension stays. `filename::sanitize` drops what Windows forbids
  and control characters, joins runs of spaces, strips trailing dots and
  spaces, and refuses an empty or reserved name such as `CON`.
- `std::fs::rename` replaces an existing target on Windows. The move uses
  `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`, so the operating system
  refuses an existing name even when the check before it loses a race. A change
  of case only is still a rename.
- The row and the `fts` row change in a transaction that commits only after the
  move worked. The hash stays, so tags, speech and jobs are untouched.
- `clipboard.rs` moved into `src/actions/` with it, as `source-layout.md`
  planned.

### Interface

- `r`, `ctrl-r` and `F2` put an input in place of the name in the side pane,
  with the stem selected and the extension beside it. Enter saves; Escape
  closes. A click on another tile closes it.
- FFmpeg opens a file without delete sharing, so Windows refuses the rename
  while the clip plays. The save drops every clip of those bytes first, then
  tries again every 50 ms for up to a second while the decoder thread closes
  the file. A file still held after that, for example by a whisper job, gives
  `the file is in use`.
- `ctrl-shift-r` starts the walk. The results become the poorly named files,
  the header says `naming 2 / 7`, and the clip plays even with autoplay off. Enter saves and moves to the next file; Enter on an unchanged name
  skips it. A click on a tile takes the walk there. Escape ends the walk and
  brings the search back. A background refresh does not replace the walk's
  results; typing a query ends the walk.

Checked in the running window, against copies of test clips in a scratch data
directory: the walk over three poor names, a refused name that already
existed with the error in the header, a rename after it with `renamed to`,
Escape back to the search, and `r`, `F2` and `ctrl-r` on a single file.
Issue 09 moves the walk into its Review tab.
