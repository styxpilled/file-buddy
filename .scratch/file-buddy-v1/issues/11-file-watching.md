# 11 - File watching and incremental rescan

Status: ready-for-agent

Depends on 03. Last, because a manual rescan covers the need until then.

## Work

- Watch the configured roots recursively with `notify`. Respect the ignore
  globs.
- A new file enters the job queue. A deleted file is marked missing, not removed
  from the database, so its tags survive if it returns.
- A moved or renamed file is matched by content hash and its path is updated.
  Nothing is processed again.
- Debounce events. A download or a large copy produces many events for one file.
- Keep a manual "rescan now" command for when the watcher misses something.
