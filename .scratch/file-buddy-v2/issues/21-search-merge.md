# 21 - Search merge and the ~ operator

Status: ready-for-agent

Depends on 20. Read the Search section of the v2 spec first.

## Work

- Load all vectors into memory at start, and keep them current as the `clip`
  stage writes new rows.
- For a query: embed it, score each file by its best frame, and keep files with
  `sim >= max(floor, top - delta)`.
- Merge with the FTS list by weighted reciprocal rank fusion, `k = 60`. The CLIP
  weight is lower than the FTS weight. It is a fifth weight on the Settings page
  and in `file-buddy.toml`. `floor` and `delta` are constants.
- FTS results show at once. Visual hits merge in after a debounce of about
  150 ms. Keep a small cache of recent query vectors.
- `~` at the start of the query searches visually only.
- The chips and the Folders scope apply to visual results.
- A visual match has the badge `visual`.
- Unit tests: the merge, the cutoff, and the `~` form.

## Measure

Tune `floor` and `delta` with the user on real queries. Record the values and
the queries here. Ask the user whether the reorder is hard to follow. If it is,
wait for both lists and show one.

## Comments
