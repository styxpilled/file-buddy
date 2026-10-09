# 41 - Stills in the Studio

Status: ready-for-agent

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
