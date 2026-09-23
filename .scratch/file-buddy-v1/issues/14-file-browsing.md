# 14 - File browsing

Status: done

Sometimes the user would like to browse the filesystem and follow the directory structure (only in tracked subdirectories).

## Work

- A new Folders page on `ctrl-2`. Review moves to `ctrl-3`, Settings to `ctrl-4`.
- The top level lists the roots from the config.
- Display the contents of the current folder.
  - Double-click or Enter on a directory navigates into it.
  - Backspace goes up one level. A breadcrumb shows the path, and each part is clickable.
- The search bar placeholder shows the current path. Typing searches only the current folder and its subfolders.
- Files that are not searchable show at the bottom, color-coded by cause. This helps to diagnose matching issues.
  - Gray: the extension does not match.
  - Red: an ignore pattern excludes it.
  - Yellow: in the database, but not probed yet.

## Comments

`search/folder.rs` lists one folder: `read_dir` for what is on disk, and
`files::in_folder` for the rows directly in it. Folders come first, then the
ready files, then the files that are not probed (`state = 'pending'`), then the
strays by cause, each group by name. `Request::within` limits a search to the
files under a folder; `files::folder_prefix` is the one place that spells the
prefix, and `rows_under` uses it too.

In the window, `results` is now `Vec<Entry>`: a folder, a file (`Hit`) or a
stray. Everything that acts on a file goes through `hit_at`, so a folder or a
stray has no preview, no marks, no tags and no rename. Copy, Reveal and Open
work on any entry. `rerun_search` follows the selection by `Entry::key`: the
hash of a file, the path of anything else.

- Enter is now `Activate`: it opens a folder and copies a file. `ctrl-c` still
  only copies.
- Backspace is `GoUp`, bound only while no input has focus.
- The Folders page keeps its folder when the user leaves it, and a folder that
  is gone, or a root that the settings took out, falls back to the nearest
  folder that still exists.
- Up a level, the folder the user came from is selected.
- The breadcrumb is gpui-component's `Breadcrumb`.
- Symbolic links are not listed, because the scan does not follow them.

The watcher does not report a change to a file that is not media, so the
listing shows a new or deleted `.txt` only after the next refresh (F5, or any
other change in the roots).

Two tests in `tests/pipeline.rs` cover the listing order and causes, the chips,
and a search inside a folder. Checked in the window: the roots list, Enter,
double click, Backspace, the breadcrumb, the placeholder, a scoped search and a
gray stray. `scripts/drive.ps1` has a `dblclick` step for that.
