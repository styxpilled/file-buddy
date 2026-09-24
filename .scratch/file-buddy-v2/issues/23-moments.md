# 23 - Moments

Status: ready-for-agent

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
