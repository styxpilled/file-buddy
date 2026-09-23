# 16 - Comprehensive refactor

Status: done

Depends on 14 and 15. Clean up and refactor the entire codebase, leaving the repository in a good state for future development. Much code was added after issue 12.

The behavior the user sees does not change. Internal behavior can change where the result is better.

Out of scope: the vendored `gpui-video`.

After this issue, we reevaluate the project goals for v2.

## Work

- Analyze the entire codebase.
- Reflect on necessity and maintainability and make decisions based on those. Think about various approaches - your first idea might not be the best one.
- Simplify unnecessary complexity.
  - Remove usages of `with_pointer`.

## Comments

Done. The user sees no change, except one bug fix: a Review tile opened from
the Folders page showed the facts of the file in place of why it is in Review.
`listing` kept its value from the Folders page. Review and Settings now reset
it. 1390 lines added, 2560 removed.

The window (`ui/mod.rs` was 2205 lines):

- Each file in `ui/` is now one part of the window: its `impl FileBuddy`
  methods beside the functions that draw it. `mod.rs` keeps the view, the keys,
  the results and the selection (826 lines). New: `player.rs`, `indexing.rs`,
  `header.rs`. `chips.rs` and `pages.rs` went into `header.rs`.
- `Mode` replaces `review: Option<Review>`, `settings: Option<Settings>` and
  `folders_open: bool`. Two pages can no longer be open at once.
- `Player` holds the ten fields of the clips and the sound. `Indexer` holds the
  seven fields of the scan, the watcher and the text stages.
- `show_results` and `update_results` replace five copies of the code that
  reset the selection, the marks and the videos. `update_results` keeps the
  tile that plays and the file opened ahead only while their row holds the
  same file. Before, a refresh could leave a tile playing another file's clip.
- One `update_index` runs both the scan and a batch of changes. The text stage
  polls with a task that is dropped when the work ends, not with an
  `Arc<AtomicBool>`.
- The rename task lives in `Renamer`, so closing the renamer cancels it. This
  removed `rename_task` and `saving: bool`.
- Review reads its four groups once per change, not twice.
- Reveal and Open no longer panic when the file goes between the check and the
  call.

`with_pointer`: gpui-kit's `Button` puts `cursor_default()` first and then the
caller's style over it, so `.cursor_pointer()` on the button works. `toggle()`
builds every on/off button, with the pointer. `drive.ps1` has a new `hover`
step that prints the cursor. With it, every enabled button checked read `hand`,
plain text `arrow` and the search box the text cursor. A disabled button was
not checked.

The library:

- A stage looks up the row of its file once and gets it as an argument. It was
  looked up two or three times per job. `stages/thumbnail.rs` was a
  pass-through and is gone.
- `FileRow::state` is the `State` enum, not a string.
- `search` reads rows with `files::read_row`, not a copy of it, and picks the
  badge by column index.
- `tags::attach` and `tags::detach` take a list of hashes. The single-file
  versions, `tags::all` and `files::indexed_filename` had no caller.
- The SQL in `fb status` and in the scan moved into `jobs::counts` and
  `fts::has_row`.

Also: removed `spike/ocr`, which the README kept until issue 10 was done.
Rewrote `docs/source-layout.md`, which listed files that no longer exist and
said SQL lived only in `db/`. Updated the README, which described the state
after issue 04.

Checked with `cargo clippy --all-targets` (clean) and `cargo test` (95 pass).
In the window: every page, Folders open, up, sort and table, Review groups and
the walk, the tag panel, the rename input, a rescan, and the cursor over the
buttons.
