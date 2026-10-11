# 45 - Dropped files, and forget

Status: done

Depends on nothing. Read the Dropped files section of the v3 spec and the File
drop part of `research.md` first.

## Work

- Take `ExternalPaths` drops on the window. GPUI already registers the drop
  target on Windows.
- A dropped file joins the index and goes through every stage. A dropped
  folder adds the media files in it and under it, once. It is not watched.
- A migration adds a column to `files` that records a dropped file. A file
  under a root is a root file, dropped or not. A dropped file under a root
  added later becomes a root file.
- The scan does not mark a dropped file missing for being outside the roots
  (the `NOT IN seen` query in `src/db/files.rs`). It checks that the file is
  on disk.
- The Folders page roots list gets a Dropped entry after the roots, while any
  dropped file exists.
- After a drop, open the Dropped entry and select the first dropped file. The
  header says how many joined. A file already in the index does not join
  again.
- Delete from database is the `forget_file` action, with no default key. It
  works on dropped files only. With marks, it skips root files and says how
  many. The file leaves the index as through `files::mark_gone`, then its row
  goes. Tags stay with the hash.

## Tests

Root or dropped for a path. The skip rule of delete from database.

## Check

Use a scratch copy with fake files, never the real roots. Drop files and a
folder from outside the roots, rescan, and confirm they stay. Delete one from
the database and confirm it stays on disk.

## Comments

### 2026-10-11

Scope grew: delete from database is now Forget, and it works on root files
too. A forgotten root file is scanned again at once. Its jobs, vectors,
thumbnail, tile and proxy go first, and its speech and text rows go unless the
user edited one, so every stage runs again from the start. This is for a file
that an error left in a bad state. A dropped file leaves the index, as the
issue said. Tags stay in both cases. The spec, its Forget section and issue 46
now say this.

- `files.dropped` is a new column (migration 13). A drop writes a path under a
  root the way the scan writes it, so a root file never gets a second row.
- The scan visits each dropped path that is on disk and not under a root, so it
  is seen and stays, and one that was missing comes back.
- `index/dropped.rs` holds `add`, `forget` and `place`. `ui/dropped.rs` holds
  the drop and the action. Forget is in the side pane's file menu, with no
  default key.
- A query on the Dropped entry searches only the dropped files.
- `sim.rs` has a `drop:x,y,path|path` step. It needs a frame between the enter
  and the submit, or the drop misses the element.

Checked in sim on a scratch copy of the dev build with its own config, root
and database, all removed after: a drop of a folder and a root file ("3 files
joined, 1 already in the index"), the Dropped entry opens with the first file
selected, a second drop joins nothing, F5 keeps the dropped files, Forget on a
dropped file leaves it on disk, and Forget on a root file gives it a new row
with every stage run again.
