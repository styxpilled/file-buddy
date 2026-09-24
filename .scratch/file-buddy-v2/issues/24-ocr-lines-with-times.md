# 24 - OCR lines with times

Status: done

Depends on 23. Read the Moments section of the v2 spec first.

## Work

- Migrate `ocr` from one text per file to one row per line, with the time of the
  frame where the line first shows.
- Run OCR again over every file. It takes minutes.
- Keep the watermark rule where the index is built, as in v1. The table keeps
  every line.
- An OCR match carries the time of its line, and seeks like a speech match.
- The Text tab lists lines with their times. A click on a time seeks. Lines the
  index leaves out stay struck through.

## Comments

### Built, 2026-09-24

- `ocr` has one row per line: hash, `time_ms` and text. `frames::sample`
  gives each frame its time, and a line takes the time of the first frame
  that shows it.
- The migration moves the lines already read into the new table with no time,
  so search keeps them until each file is read again. It then queues OCR
  for every file that was done.
- An OCR match finds its line as a speech match finds its segment, among the
  lines the index keeps. A still has no time.
- The Text tab lists the lines with their times, and a click on a time seeks.
  The lines the index leaves out stay struck through.

### The pass over the real collection, 2026-09-24

On a copy of the database: the migration kept all 22,068 lines of the 2,129
files that had text, and queued 3,338 files. The OCR pass then took 6 minutes
(362 s), with 3,338 done and none failed. It gave the same 22,068 lines, all
with times. Three random lines, searched with `fb search`, each gave a screen
text match at the time of the line.

The real database migrates on the next start of the window, which then reads
every file again in the background.

Checked in the window, on the copy: the Text tab shows the lines with their
times, and a click on 0:04 seeks to 0:04.
