# 22 - More like this

Status: done

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

### Built, 2026-09-24

- `Vectors::like` takes the mean of each file's frames, then the mean of the
  files, so a video of ten frames counts as much as a still of one. It scores
  every other file by its best frame.
- `l` works on Browse and Folders. Each `l` saves the results, the selection
  and the marks it replaces, so escape brings them back exactly. Escape clears
  new marks first, then goes back one step. Typing in the box ends every step.
- The search runs in the background, and again when the index or the chips
  change, as a typed query does.
- Escape after an `l` that came before the visual hits of a typed query
  merged in runs that merge again.
- `fb like <path>...` prints the scores and the times. In a debug build,
  scoring 31,514 vectors takes 185 ms and finding the files in SQLite 25 ms,
  both in the background.

### Cutoff, 2026-09-24

The cutoff from 21 does not fit. Between two files, scores run from 0.6 to
0.97, and `top - 0.03` kept 1 to 4 files for most queries. Files kept by delta,
for the top file of 8 topic queries (first 25 only):

| File | top | 0.03 | 0.05 | 0.08 | 0.10 | 0.15 |
| --- | --- | --- | --- | --- | --- | --- |
| a cat | 0.960 | 4 | 14 | 25+ | 25+ | 25+ |
| a dog | 0.823 | 4 | 5 | 7 | 17 | 25+ |
| a car | 0.798 | 1 | 1 | 2 | 4 | 25+ |
| snow | 0.782 | 1 | 2 | 2 | 2 | 11 |
| a video game | 0.742 | 3 | 4 | 6 | 8 | 25+ |
| a man talking to the camera | 0.970 | 1 | 1 | 1 | 2 | 5 |
| food | 0.683 | 3 | 3 | 16 | 25+ | 25+ |
| a meme with a caption | 0.700 | 7 | 20 | 25+ | 25+ | 25+ |

No one delta fits them all, and a near copy of the query file pulls `top` up.
The user chose a fixed count: the 50 best files, after the chips and the
folder. The query is a real file, so a query that nothing fits cannot happen.

Checked in the window on the real collection: `l` on a cat clip gives 50 cat
clips, `l` with two files marked shows `like: 2 files`, and escape goes back
through both steps to the typed query.
