# 09 - Review screen

Status: done

Depends on 03 and 08. Four kinds of file need a person to look at them, and they
all want the same screen: a list, a preview and a key to open Explorer.

The review screen should be accessible from a tab switcher in the top. One tab for regular browsing (pick a name), one tab for the review screen, one tab for a future settings page.

## Groups

- Duplicate files, grouped by content hash. The hash makes this free.
- Files longer than 5 minutes. They are outliers. The user deletes them outside
  the application.
- Files that could not be indexed, with the error text.
- Files with no proper name. This tab is the bulk walk from issue 08.

## Rules

The application never deletes a file. It shows the file and opens Explorer. The
decision and the action stay with the user.

## Comments

Done. Review is a mode of the main window, not a second screen: the grid, the
side pane, reveal, open and rename work in it with no changes.

### Database

`db::review::items` gives the files of one group, each with a note that says why
it is there.

- Duplicates: every path whose hash another path holds, ordered by hash and then
  path, noted `copy 1 of 3  ·  folder`.
- Long: longer than `LONG_FILE_SECONDS`, the longest first, noted `12:04`.
- Failed: `state = 'failed'`, noted with the recorded error.
- Poor names: the rule from issue 08, which moved here from `db/files.rs`.

The note holds the last folder only. The root that every path shares fills a
tile and says nothing; the side pane shows the whole folder.

### Interface

- Review opens on the first group that holds a file. `alt-1` to `alt-4`, or a
  click on a tab, picks a group, and opens Review when it is closed. The walk from issue 08 has no key
  of its own now: it is the Poor names group.
- The tabs, with a count each, take the place of the search box and the chips.
  `f` does nothing while Review is open. A tile shows its note in place of the
  signal badge, because nothing matched. The header counts the files, or says
  `naming 2 / 7` in Poor names.
- A background scan or a transcript refresh does not replace the group in view.
  The counts update after a rename.
- A rename outside Poor names loads the group again and keeps the selection on
  the file. In Poor names it moves on to the next file, as the walk did.
- Escape closes the rename input first, then leaves Review. Leaving Review runs
  the search again.
- The application deletes nothing. `ctrl-e` reveals the file in Explorer.

Checked in the running window against scratch copies: two copies of one clip in
different folders, a 310 second file, random bytes named `.mp4`, and a poor name.
Each group showed its files and notes, the failure showed its full error in
the pane, a rename in Duplicates stayed in the group, the walk renamed the poor
name and its count went to 0, and Escape went back to the search.

### Follow-up: pages

Review was a mode with no way in except a key. The header now starts with three
page tabs, Browse, Review and Settings, on the same row as the search box or the
group tabs. `ctrl-1` to `ctrl-3` switch pages, so the group keys moved to
`alt-1` to `alt-4`, and `ctrl-shift-r` is gone. The header has one fixed height,
so the body does not move when the search box comes and goes.

Settings edits `file-buddy.toml`, which the spec had ruled out:

- Folders, with Add folder (the system folder picker) and Remove; the ignore
  globs, one per line; the four weights.
- `Config::edited` checks the form before anything is written: at least one
  folder, each weight a number of 0 or more, each glob valid. The error shows in
  the header and the file stays as it was.
- `ctrl-s` or Save writes the file. Weights reach the next search. A change of
  folders or globs starts a scan; a scan that is already running finishes
  first, and transcription is never started twice.
- Revert reloads the form from the saved configuration. Opening Settings stops
  every clip and empties the results, so no file key acts on a file that is not
  on screen.
- Escape takes focus out of a field first, then goes back to Browse.

Checked in the running window against a scratch copy: the three pages, a weight
of `abc` refused, a weight of `12` saved to the TOML file, removing the only
folder refused, and Escape back to Browse. A save that changes folders was not
checked in the window, because the folder picker is a system dialog; it runs the
same scan as the start of the application.
