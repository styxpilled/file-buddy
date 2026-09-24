# 28 - Browse the whole collection

Status: ready-for-agent

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
