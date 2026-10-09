# 40 - Text

Status: done

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

### 2026-10-09

Done, with `drawtext`. The overlay fallback is not needed.

Measured first. `drawtext` with Arial Bold at 200 px and `y_align=font` puts
the ascent at `y`, and its line advance is 1.15 em: ascent, descent and line
gap. `text_h` is that advance times the number of lines, and the box is
`text_w x text_h` plus the border on each side. GPUI puts half of the line
gap above the ascent when the line height is 1.15 em, so the export moves
`y` down by that much: 0.016 em.

Then the preview against the export, at an output of 1080 x 674, 54 px, two
lines, with the box: the box, the glyphs and both lines agree to within 2
output pixels. Part of that is the measure of the frame edge in the picture.

- Text times are in source time, as the range is, so a text stays on its
  moment when the range or the speed changes. The export clips them to the
  range. The filter comes before `setpts`, so `enable` takes the time from the
  in point with no speed change.
- The size is a fraction of the output height, rounded to whole pixels as
  `drawtext` needs. The panel offers 4, 6, 8, 10, 14 and 20%. The box padding
  is 0.25 em.
- A value is escaped twice, for the filter graph and then for the filter
  options. `expansion=none` keeps `%{...}` as typed. The escaped `text=`
  draws the same pixels as `textfile=` with colons, quotes, backslashes,
  commas, brackets, a semicolon, `%{n}` and a newline in the words.
- The words lose their spaces at both ends, because FFmpeg drops them.
- The selected text shows faint outside its time, so it can still be dragged.
  A click on a bar under the seek bar selects its text. A drag on either end
  of the bar seeks the preview there.
- `n` puts the keys in the words input with the words selected. Enter gives
  the keys back to the Studio, and `shift-enter` adds a line.
- `delete` in the Studio removes the selected text. With no text selected it
  does nothing there, where it used to trash the selected file.
- `ui/studio.rs` became `ui/studio/mod.rs`, with the texts in
  `ui/studio/text.rs`.

### 2026-10-09, review

Status of this comment: done. The stages:

1. `delete` selects the next text, or the one before when none is next.
2. Timeline bars: a fainter fill, the words on the bar, ends in another
   color, a little taller for the words, no line inside the bar.
3. A drag on the middle of a bar moves the text through time with no seek.
   A drag on an end does not seek either, and the end snaps to the playhead
   when it comes near.
4. The panel splits into tabs: the edit and the export in one, the texts in
   the other.
5. The text tab lists the texts as the Speech tab lists segments.
6. The size presets read as names with the size in pixels, not as percents.
7. With no crop, the crop controls are faint.

All seven are done. The tabs are Edit and Text. A text selected on the
preview or the timeline opens the Text tab, and a change of tab gives the
keys back to the Studio. The sizes are Small, Medium, Large and Huge, at 5,
8, 12 and 18% of the output height, and the panel shows the size in pixels
beside them. An end snaps when it comes within 6 px of the playhead. The
crop controls go to half opacity while the crop is the whole frame and the
aspect is free.

### 2026-10-09, second review

Done:

- The Text tab has no count. Each row shows the start and end time on its
  first line and the words under it. A double click edits the words in
  place, as a segment of the Speech tab is edited. Enter, escape or a click
  elsewhere ends the edit. The list scrolls, and a new or selected text
  scrolls into view.
- The textarea has no size option and keeps the padding of a full size
  input. A wrapper with negative margins takes it back, and its height comes
  from the line count, because by itself it keeps room for one line more.
- The timeline always keeps room for five rows. More rows scroll.
- The crop controls in the panel have their normal color again. The crop
  box on the preview is dim while it holds the whole frame: grey grips and a
  faint border.
