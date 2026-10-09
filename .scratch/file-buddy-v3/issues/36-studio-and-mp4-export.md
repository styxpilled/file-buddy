# 36 - The Studio page, the range and MP4 export

Status: ready-for-agent

Depends on nothing. Read the Studio, Range, Export and Export folder sections
of the v3 spec first.

This is the base that 37 to 44, 50 and 51 build on.

## Work

- `export_folder` in `file-buddy.toml`, and a field for it on the Settings
  page. Unset, export does nothing and the header says to set one.
- A fifth page, Studio, with a header tab. `ctrl-5` switches to it and joins
  the fixed keys. `c` opens the selection in it, as a new history step.
- One edit, held until another source replaces it. Nothing goes to the
  database.
- A large preview that plays the source, the panel of controls beside it, and
  the timeline under it. Reuse the v2 seek bar and give it two handles.
- `[` and `]` set the in and out points at the playhead. A drag on a handle
  moves it and seeks to it. Playback loops over the range. The panel shows the
  length of the range.
- `ctrl-enter` exports MP4: `h264_nvenc` with AAC, and a quality control.
- One function builds the FFmpeg arguments from the edit, with no side
  effects. Later issues add crop, scale, text and speed to it, in the order of
  the spec: trim, crop, scale, text, speed.
- The export runs as a subprocess, with `-nostdin`, and the window does not
  wait. The page shows progress. One export at a time.
- When it ends, the file goes to the clipboard as `CF_HDROP` and path text,
  through `actions::clipboard::copy_file`. The header confirms it and shows
  the size.
- The name is `<source stem> <in>-<out>.<ext>`, for example
  `cat jump 0m03-0m07.mp4`. A name that exists gets ` (2)`, ` (3)`, and so on.
- A failure logs the FFmpeg error, and the header says the export failed.
- The Studio keys are a context inside the window, as the Speech and Text rows
  are, so its keys win there. Add the actions to the custom keys list.

## Tests

The arguments for a range export. The export name and its collision suffix.

## Check

Export a range of a test clip. It pastes into Explorer and into Discord, and
it plays from the in point to the out point with sound.

## Comments
