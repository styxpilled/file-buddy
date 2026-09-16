# 06 - Tags

Status: done

Depends on 04.

## Work

- A key opens a tag input on the selected file, with autocomplete over the tags
  that already exist. Enter applies. Escape closes.
- Tags are lowercase. Autocomplete is what stops `funny`, `Funny` and `funnny`
  from becoming three tags and making the feature useless.
- Several selected files can be tagged at once.
- Tags feed the highest weighted FTS column, so a bare word finds them with no
  syntax.
- The `tag:` operator limits a term to tags, for a common word that filename
  matches would otherwise bury.

## Comments

Done. The search side already worked: `fts` has indexed the tags column since
issue 03, and `tag:` has been in `query::build_match` since issue 04. This was
the writing side and the interface.

### Database

- `attach_many` and `detach_many` take a list of hashes and run in one
  transaction. Each file rebuilds its own FTS row, so a failure halfway through
  a loop at the caller would leave some files tagged, some indexed, and no way
  to tell which.
- `matching` is the autocomplete query: it matches anywhere in the name, but
  ranks a prefix first, then the most used tag. `%` and `_` in a tag are
  escaped, so a tag holding a wildcard is text and not syntax.
- `common_to` gives the tags every marked file has. Only those can be taken off
  the whole set in one action.
- `detach_many` deletes a tag that is left on no files. A name that nothing uses
  can only clutter the one list that has to be trustworthy.

### Interface

- `t` opens the panel on the marked files, or on the selection when nothing is
  marked. `x` marks the selection and ctrl-click marks a tile. A mark is held by
  hash, not by grid index, so it survives a re-search; a new query clears the
  marks, because they were a promise about files the user could see.
- Escape undoes one thing at a time, the most recent first: the tag panel, then
  focus in the search box, then the marks. `input::Escape` only reaches this
  view while an input has focus, so escape needs a second binding of its own for
  the rest of the time. There is no separate key for clearing marks: escape is
  the key a hand already reaches for.
- The panel lists the matching tags as you type, with the first highlighted.
  `alt-up` and `alt-down` move that highlight. Plain arrows, tab, enter and
  escape are all bound inside gpui-kit's own `Input` context, which is deeper on
  the focus path than anything this application can put around it, so the
  arrows were never available. Off either end of the list the highlight goes to
  nothing, which is how a brand new tag gets made.
- Enter goes through `InputEvent::PressEnter` rather than the `Enter` action,
  which is the route gpui-kit documents.
- The panel stays open and empties after each tag, because a file usually wants
  more than one. It is `occlude`d, or a click on a suggestion would land on the
  tile behind it as well, and the tiles under it would start playing on hover.
  A click outside it closes it.
- The tags common to the targets show as chips; clicking one takes it off all of
  them. The issue did not ask for removal, but a tag feature with no way to undo
  a saved typo is a feature that makes the index worse.
- A tag changes the ranking, so the query runs again. `rerun_search` keeps the
  selection where it was: dropping the user back at the top of the grid in the
  middle of tagging would lose their place.

Checked in the running window: marking two files, `t`, typing `reaction`, Enter,
then `tag:reaction` returning exactly those two with the tag badge. Typing
`react` afterwards suggested `reaction`; `alt-down` moved the highlight off it
and `alt-up` back. Clicking the chip took the tag off both files. The test tag
was removed again afterwards, so the database is as it was.

### Follow-up

Five faults found in use, all fixed and checked in the window again:

- Escape closed the panel only while the input had focus.
- A click on a suggestion reached the tile underneath.
- A click outside the panel did not close it.
- `ctrl-shift-a` for clearing marks was an awkward key. It is gone.
- Escape now clears the marks when there is nothing else to dismiss.
