# 25 - Editing speech and screen text

Status: done

Depends on 24, so both tabs hold rows.

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

### Built, 2026-09-24

- `segments` and `ocr` have an `edited` flag. A deleted row keeps its place
  with empty text. `db::edits` changes, deletes and adds rows, and rebuilds the
  index row of the file at once.
- A file with an edited row skips the stage: `stages::run` does not run it,
  and `replace` keeps the rows if an edit lands while the stage runs. An edit
  matches its row by id, hash and the text it had, because SQLite gives a new
  row the id of a deleted one.
- `ocr::indexed` is the one rule for the index, the search moment and the
  strike-through: an edited line is always indexed.
- The migration header now names the edited rows next to the tags.
- Keys, agreed with the user: `i` cycles the tabs, `e` puts a cursor on the
  rows, up and down move it and seek, enter edits, `delete` deletes, `l` adds
  at the time of the clip. More like this moved from `l` to `a`. Escape closes
  the row input, then the cursor, after the tag panel and the rename input.
- `e` refuses a tab whose stage has not run yet, and says so in the header.
  An added row would otherwise stop the file from ever being transcribed.
  Enter on an empty tab adds a row.
- Focus that leaves the rows or their input ends the editing and drops what
  was typed, so a click outside cannot leave the arrows on the rows.

Checked in the window, on a copy of the database: an edit, a delete and an
added line show at once, and a search for the added line finds the file with
a screen text match at its time. `e` on a file not yet transcribed says so.
