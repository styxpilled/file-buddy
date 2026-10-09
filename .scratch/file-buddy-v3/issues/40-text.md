# 40 - Text

Status: ready-for-agent

Depends on 38, because text positions are fractions of the frame after crop
and scale. Read the Text section of the v3 spec and its open questions first.

## Work

- `n` adds a text at the center, for the whole range.
- A click on a text in the preview selects it, a drag moves it.
- The panel edits the words, the size and the box toggle. `shift-enter` adds a
  line.
- Each text has a timeline row with a bar from its start to its end. A drag
  on either end changes it.
- `delete` removes the selected text when no input has focus.
- Arial Bold, `C:/Windows/Fonts/arialbd.ttf`, white, with an optional black
  box with padding.
- Position and size are fractions of the output frame.
- The preview draws the text in GPUI with the same font file. The export uses
  `drawtext` with an `enable` time range. The font path needs the quoting
  that `research.md` records.

## First, measure

Draw the same text at the same size both ways and compare. If they differ by
more than a few pixels, render each text to an RGBA image in the application
and burn it in with `overlay`, so the preview and the export share pixels.
Record the result here.

## Tests

The position and size math after a crop and a scale. The `drawtext`
arguments, with the escaping of colons, quotes and backslashes in the words.

## Comments
