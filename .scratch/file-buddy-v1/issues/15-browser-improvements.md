# 15 - Browser improvements

Status: done

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

## Comments

`search::folder::Sort` is Name, Recent (newest first) or Size (largest
first). `folder::list` takes it and sorts each group with it; ties go by name.
The groups keep their order: folders, indexed files, files not probed, strays.
Folders stay by name in every mode, because the time of a folder changes only
when a file directly in it changes. The sort does not touch a query, and its
buttons show only while the page lists a folder.

`FileRow` now has `mtime`, which the table shows and the sort reads. A stray
has no row, so `Entry::Stray` carries the size and time from `read_dir`.

`ui/table.rs` is the table: a header and a virtual list of 26 px rows, one
column for the keys. A click goes through `tile::click`, the same as a tile.
Tags come from `tags::by_hash` in one query, loaded only while the table
shows. The time is local, through `SystemTimeToTzSpecificLocalTime`, so the
`windows` crate has the `Win32_System_Time` feature now. The tags column is
one line of names in the tag colour, cut at its width.

Keys: `s` steps through the sorts, `v` switches the view. The sort buttons are
at the right of the breadcrumb bar. Both live in `FileBuddy` and reset on
restart.

Follow-up: the table is on Browse and Review too, with one view for all three
pages. The Grid and Table buttons moved to the header, after the chips or the
Review group tabs. In Review the last column is "Why", the note the tile shows,
because a duplicate or a failure means nothing without it.

Checked in the window: the grid and the table, each sort with the selection
kept, a query in the table, and a click on a row. A test in
`tests/pipeline.rs` covers the three orders and the groups.
