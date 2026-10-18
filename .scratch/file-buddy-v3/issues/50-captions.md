# 50 - Captions from the transcript

Status: done

Depends on 40. Read the Captions section of the v3 spec first.

## Work

- Add captions, in the Studio panel, for a source whose speech stage has run.
  If it has not run, the header says so.
- Each segment inside the range becomes one text, with its start and end in
  output time, cut to the range ends. Speed changes output time.
- Edited segments apply. Deleted ones, with empty text, do not show.
- Bottom center, one size, box on. After that they are ordinary texts.
- Wrap to at most two lines that fit 90% of the frame width, measured with the
  export font. No word-by-word highlight.

## Tests

Segments across the range ends, at a speed other than 1x. The line wrap.

## Comments

Done. Captions are not on when the Studio opens. The Captions button beside
(N)ew in the Text section adds them. It shows only for a video source.

- `studio/captions.rs` makes the texts. Text times stay in source time, as
  every text has, so the export puts them at the right output time at any
  speed. A segment is cut to the range ends.
- Size 5% (Small), box on, centered. The bottom of the box is at 92% of the
  frame height, so a one-line and a two-line caption share a bottom edge.
- The lines wrap at 90% of the output width, measured with GPUI's
  `shape_line` in Arial Bold. A segment longer than two lines becomes
  consecutive captions, and they share its time by the length of their words.
  A word wider than the line has a line of its own.
- With no finished speech stage, the header says why, with the same words
  as the Speech tab. With no segment in the range, it says
  "no speech in the range".
- Tests: segments across both range ends at 2x, the wrap, the split of a long
  segment, a word wider than the line.
- Checked with sim: a 20 s video with two segments gets two captions at
  0:02 and 0:18. With a 9:16 crop the second wraps and splits in two.
