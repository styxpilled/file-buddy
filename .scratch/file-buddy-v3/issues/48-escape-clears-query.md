# 48 - Escape clears the query

Status: done

Depends on nothing. Read the Escape clears the query section of the v3 spec
first.

## Work

In `dismiss` in `src/ui/mod.rs`, after the last "more like this" step: if the
search box holds text, clear it and run the search. Focus can be outside the
box. It makes no history step.

## Tests

The escape order, if `dismiss` can be tested without a window. If not, check
it in the window and say so here.

## Comments

`dismiss` needs a `Window`, so it has no unit test. Checked in the window with
sim: with "lebron" typed in the box, the first Escape takes focus out of the
box, the second clears it (22 results back to 4021), and a third does nothing.
Alt-left after the clear does not bring the query back, so it made no history
step.
