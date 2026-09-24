# 23 - Moments

Status: done

Depends on 21 for visual times. The speech part can come first. Read the
Moments section of the v2 spec first.

## Work

- A speech match carries the time of the segment that matched. FTS5 gives only
  the file, so find the segment that holds the query terms.
- A visual match carries the time of its best frame.
- The side pane opens the clip at that time.
- The tile snippet starts with the time: `0:42 the line you said`. A visual
  match shows the time alone.
- The Speech tab lists segments with their times. A click on a time seeks.
- A result is still a file. Do not rank moments.
- Unit tests: the time format, and finding the segment that matched.

OCR times come in 24.

## Comments

### Built, 2026-09-24

- `Hit::at_ms` is the time of a match in its clip. A speech match takes the
  start of the segment that holds the most words of the query, with the same
  prefix rule as the index, and shows that segment as its text. A visual
  match takes its best frame. A still has no time.
- The pane seeks to the time when it opens the clip, with the accurate seek
  from 29. A clip already in the pane stays where it is, so a new keystroke
  does not move it.
- A click on a time in the Speech tab seeks there.
- `fb search` prints the time of each match.

Finding the segments costs one query per speech match. On the real
collection in a debug build, a one-letter or two-letter query that matches
120 to 260 transcripts spends 4 to 10 ms of its 15 to 21 ms there. A longer
query spends under 0.5 ms.

Checked in the window: the tiles show `speech 0:00 Just see those sleigh
bells...` and `visual 0:02`, the pane opens visual matches at 0:02 and at 2:37,
and a click on 0:11 in the Speech tab seeks to 0:11. From the command line,
two lines in the middle of one clip give speech matches at 0:11 and 0:15.
