# 22 - More like this

Status: ready-for-agent

Depends on 21. Read the More like this section of the v2 spec first.

## Work

- `l` searches with the mean of the selection's frame vectors. With files
  marked, use the mean over all marked files.
- Each file scores by its best frame. Leave out the query files. The cutoff from
  21 applies.
- The search box shows `like: <name>` or `like: <n> files`. Escape goes back to
  the results from before, as one more step in the escape order.
- The chips filter the results.
- Unit tests: the mean of several files, and that the query files are left out.

## Comments
