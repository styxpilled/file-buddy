# 21 - Search merge and the ~ operator

Status: done

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

### Built, 2026-09-24

- `search::visual` holds every vector in memory. The window loads them in the
  background at start, and before each visual search it reads only the rows
  with a higher id than the last one it read.
- Each file scores by its best frame. The cutoff is `max(FLOOR, top - DELTA)`,
  starting at `FLOOR = 0.25` and `DELTA = 0.03`, from the topic queries in 20.
- RRF with `k = 60`. The text list has weight 1. The visual weight is the fifth
  weight, `weights.visual` in `file-buddy.toml`, 0.5 by default. A file in both
  lists keeps its text badge.
- The text matches show at once. After 150 ms, a background task embeds the
  query (64 queries are cached), scores it and merges. The next keystroke drops
  the task.
- If the selection is still on the first result, it stays there when the
  visual hits merge in. Following the old first file moved the grid.
- `~` searches only visually. The words that go to the text encoder leave out
  `~` and `tag:` terms.
- `fb search` merges the same way. `fb visual <query>` prints the scores and
  where the cutoff falls.

Scoring the 31,514 vectors takes about 190 ms in a debug build. It runs in the
background.

### Result, 2026-09-24

The user tested the window on the real collection and is happy with the
results. `FLOOR` stays at 0.25 and `DELTA` at 0.03, and the visual hits keep
merging in after the text matches. The user saw what may be a memory leak.
Issue 32 investigates it.

