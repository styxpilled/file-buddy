# 50 - Captions from the transcript

Status: ready-for-agent

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
