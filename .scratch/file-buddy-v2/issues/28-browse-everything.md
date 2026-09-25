# 28 - Browse the whole collection

Status: done

Depends on nothing.

With nothing typed, Browse shows only the 500 newest files. The user wants to
scroll through the whole collection, and to sort it the way the Folders page
sorts a folder.

## Cause

`search::Request::new` sets `limit: 500` (`src/search/mod.rs`), and `browse`
passes it to `LIMIT`. A typed query uses the same limit.

## Work

- Browse with no query lists every present file that the chips allow, newest
  first. No limit.
- A typed query keeps its limit of 500. A query that matches more than 500
  files has not found anything.
- The header count shows the real number of files.

## Sort

- With nothing typed, `s` and the sort buttons sort Browse by name, by time
  (newest first) or by size (largest first). Reuse `search::folder::Sort`. Ties
  go by name, as in a folder. Sort in the SQL, not in Rust.
- Browse keeps its own sort, and it starts on Recent, which is the order Browse
  has today. Folders keeps its own sort, which starts on Name. Both last until
  the application closes.
- `set_sort` in `src/ui/folders.rs` returns early on any page but Folders.
  Change that.
- Browse gets a bar above the grid, with the same height and style as the
  Folders breadcrumb bar. Build the two from one piece of code. Later work will
  put more in this bar. For now, the left side says `All files`, and the sort
  buttons sit at the right. The bar stays while a query is typed, but the sort
  buttons go, as on Folders.
- A typed query keeps its ranking.

## Measure

At the real collection size, with the grid and with the table:

- time to fetch and show Browse,
- time of the refresh that brings new text into the results every 30 s,
- scrolling from the top to the bottom.

The grid and the table are virtual lists, so only the rows in view are drawn.
If a number is bad, record it here and fix that one thing. Do not add paging
without a measured reason.

## Comments

### Done (2026-09-25)

- `browse` has no LIMIT now. `Request` has a `sort`, which only Browse uses.
  The SQL orders by the sort, then by `lower(filename)`, then by path.
  `lower()` folds ASCII letters only, and the folder sort folds all of
  Unicode. So two names that differ only in the case of a non-ASCII letter
  can tie in a different order. This did not seem worth a custom collation.
- `FileBuddy` keeps `browse_sort` (starts on Recent) and `folder_sort` (starts
  on Name). `s` and the buttons change the sort of the page that is open.
- The bar was already one piece of code for every page (`header::bar`).
  Browse puts `All files` at the left and the sort buttons at the right. The
  buttons go while a query is typed.

### Measured

3427 files, debug build, 60 Hz display.

| What | Result |
| --- | --- |
| Fetch and show Browse | 8 to 10 ms |
| Refresh (`rerun_search`, the same path as the 30 s refresh) | 8 to 16 ms |
| Grid: scrollbar drag top to bottom, then 4 s of autoscroll | median frame 16.5 ms, p95 17.0 ms, max 49 ms |
| Table: the same | median 16.5 ms, p95 17.0 ms, max 49 ms |

No number is bad, so there is no paging.
