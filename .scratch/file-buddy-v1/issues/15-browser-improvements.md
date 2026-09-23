# 15 - Browser improvements

Status: ready-for-agent

Depends on 14. Applies to the Folders page only. Search results keep relevance order.

## Work

- Add alternate sorting modes for the folder view:
  - Recent (file modified time)
  - Alphabetical
  - Filesize
- Add a concise table view that does not display preview tiles.
  - Columns: name, kind, length, size, modified, tags.
  - Keep the tags column compact. It must not add visual clutter.
- Sort mode and view type are kept in memory only. They reset on restart.
