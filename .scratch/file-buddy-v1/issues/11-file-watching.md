# 11 - File watching and incremental rescan

Status: done

Depends on 03. Last, because a manual rescan covers the need until then.

## Work

- Watch the configured roots recursively with `notify`. Respect the ignore
  globs.
- A new file enters the job queue. A deleted file is marked missing, not removed
  from the database, so its tags survive if it returns.
- A moved or renamed file is matched by content hash and its path is updated.
  Nothing is processed again.
- Debounce events. A download or a large copy produces many events for one file.
- Keep a manual "rescan now" command for when the watcher misses something bound to the F5 key.

## Comments

Built on `notify` 8.2.0. `index/watch.rs` holds the watcher, the debounce thread
and `apply`, the incremental scan over one batch.

- Events go to a thread that collects paths until the roots are quiet for 1.2 s,
  or 10 s pass, whichever comes first. A download or a folder copy is one batch.
- The thread drops what the ignore globs match and what is not a media file,
  unless the path is gone: a deleted folder is one event for the folder.
- A watcher path is rewritten against the root as the configuration spells it,
  because the watcher says `D:\clips\a.mp4` where a root of `D:/clips` puts
  `D:/clips\a.mp4` in the database.
- The window polls the watcher every 500 ms and holds a batch while a scan runs.
  A batch that is indexed reports "2 new, 1 moved" in the header.
- A file that is still being written fails to hash. It comes back on the next
  batch, and after a second failure only a rescan brings it in.
- `f5` rescans.

A gone file keeps its row, marked `missing_at`. Its tags and its finished stages
come back with it, and the search, the browse list and all four Review groups
hide it. A row whose bytes a present path still holds carries nothing of its
own, so that one is deleted instead: this is what a move looks like to a full
scan. `files::remove_missing` became `files::mark_missing`, and
`ScanStats::removed` became `ScanStats::missing`.

An incremental move never creates the second row: the batch holds both paths, so
the new path takes the row of the old one by content hash and no stage runs
again.

Five tests in `tests/pipeline.rs` cover the new file, the move, the deleted
folder, the tags of a deleted file that comes back, and a live watcher reporting
a real copy. The window wiring is compile checked only.
