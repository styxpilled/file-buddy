# 24 - OCR lines with times

Status: ready-for-agent

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
