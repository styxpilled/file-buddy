# 34 - Delete

Status: done

Depends on nothing. Read the Delete section of the v2 spec first. It lifts the
v1 non-goal "The application never deletes a file."

`delete` moves the marked files, or the selection, to the recycle bin.
`shift-delete` deletes them for good, after the Windows confirmation dialog.
The file menu gets Trash and Delete, which do the same as the keys.

## Comments

### Built (2026-09-26)

- `actions::delete` runs `IFileOperation` on a thread of its own, in a
  single-threaded apartment, so the shell's dialogs do not run inside the
  window's message loop. Before it starts, it waits up to 1 s for each file to
  open with delete access, because a decoder closes its file a moment after
  its clip is dropped.
- Trash uses `FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOFX_RECYCLEONDELETE`, and
  `FOF_WANTNUKEWARNING`, so the shell asks before a file that cannot go to the
  recycle bin is deleted for good. Delete sets no flags, so the shell asks
  first.
- A cancelled dialog is not an error. What counts afterwards is which files are
  no longer on disk: each leaves the index with `files::mark_gone`, as it would
  when the watcher finds it gone.
- The shell rejects a path with `/`, and a root from the config can mix the
  separators. The path goes to the shell with backslashes only.
- `delete` is also the default key of `delete_row`. Key conflicts are now
  checked per context, and the rows' context wins inside the window.

Checked on a copy of the application with its own config, database and six
generated clips, never the user's roots: trash of the selection while it
played, trash of two marked files, `shift-delete` cancelled and confirmed (not
in the recycle bin afterwards), and Trash and Delete from the menu.

A mark was by hash, so marking one copy of a duplicate marked every copy, and
a delete took them all. A mark is now on one file, by its row id. Tags and "more
like this" still act on the bytes, so two marked copies are one target there.
Not tested in the window.
