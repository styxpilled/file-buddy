# 56 - A design system based on Adobe Spectrum

Status: needs-triage

Depends on nothing. Design work, so the choices go to the user before code.

## Why

The colors come from the `gpui-kit` dark theme as it is. The only color the
user chose is the accent of the Studio range. The elements were built one by
one, with no shared rules for size, space, color or state.

## The accent

The accent is `#30a7fe`. Its muted form is the same color at 30% opacity. Today
only the Studio range uses it: the in and out handles, and the band between
them (`RANGE_COLOR` in `src/ui/seek_bar.rs`). New work that needs an accent
uses it.

## Work

- Read Adobe Spectrum (Spectrum 2): the color system, the scales of size and
  space, the type scale, and the components the application has: buttons,
  toggle buttons, text fields, menus, tabs, sliders, tables and the progress
  bar.
- Make a palette from Spectrum's dark theme around the accent: the gray scale
  of the backgrounds and borders, the text colors, and the colors for the
  states (hover, down, selected, focus, disabled) and for the meanings
  (error, warning, success).
- Put the tokens in one place in the code. Set them on the `gpui-kit` theme
  where it has a slot for them, so the components take them, and use the
  rest directly. No hex values in the views.
- Redesign the elements that the user picks. Candidates: the header tabs, the
  kind chips, the tiles and the selection, the table rows, the side pane's top
  row, the seek bar, the Studio panel and its toggles, and the flash in the
  header.
- Replace the current uses of `theme.primary` and `theme.warning` that mean
  "accent" or "selected", for example the seek bar fill, the export progress
  and the selected table row.

## Open questions

- Which elements get a redesign, and which only get the new colors?
- Does the seek bar fill take the accent, or stay neutral so that the range
  stands out?
- Spectrum's own font is Adobe Clean. Keep the system font, or pick another?

## Check

Every page in the sim build, before and after, with the user. Text and
controls keep a contrast of at least 4.5:1 on their backgrounds.

## Comments
