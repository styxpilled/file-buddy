# 41 - Stills in the Studio

Status: done

Depends on 38 and 40. Read the Stills section of the v3 spec first.

## Work

- A still opens in the Studio with no timeline and no speed.
- Crop, scale and text apply. Text has no time range.
- Export is PNG or JPG. JPG has a quality control.
- The export name has no range: `<source stem>.<ext>`, with the same
  collision suffix.

## Tests

The arguments for a still with crop and text.

## Comments

### 2026-10-10

Done.

- `c` on a still opens it in the Studio. ffprobe gives the size, and the
  stage draws the file with GPUI `img`, letterboxed as the video is, so the
  crop box and the texts line up.
- The page has no seek bar and no text rows, and the stage takes their
  height. The panel has no In and Out row and no Speed. The row above the
  stage keeps only the file menu.
- A text on a still always shows. The Text tab rows have no times, and the
  export has no `enable`.
- The format picker shows PNG and JPG. JPG has the six quality levels, as
  `mjpeg -q:v` 2, 3, 5, 8, 12 and 18, with `yuvj444p`. PNG keeps alpha.
  A still and a moving source each keep the format last chosen for them.
- The output is `-frames:v 1 -update 1 -f image2`. `-update` keeps a `%` in
  the name from reading as a pattern.
- The size stays even, as for video. This takes at most one pixel off an
  edge.
- GPUI does not decode AVIF. An AVIF still (one in the index) shows an
  empty stage, but crop, text and export still work.
