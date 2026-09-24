# 25 - Editing speech and screen text

Status: ready-for-agent

Depends on 24, so both tabs hold rows. Read the Editing section of the v2 spec
first.

## Work

- In the Speech and Text tabs: edit, delete and add a segment or a line. The
  change reaches the index at once.
- Add an `edited` flag per row. A deleted row stays, with empty text.
- A file with an edited row skips any new run of that stage.
- Migrations must never drop edits. Treat them like tags.
- An edited OCR line is always indexed, even if the watermark rule would leave
  it out.
- Keyboard first. Propose the keys to the user before building them, and fit
  them into the escape order.
- Unit tests: the re-run rule, and the stoplist bypass.

## Comments
