# 48 - Escape clears the query

Status: ready-for-agent

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
