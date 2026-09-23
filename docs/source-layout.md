# Source layout

Where code goes, and why. Read this before you add a file to `src/`.

## The rule

A directory is a purpose, not a layer. Files that serve one purpose live
together, and the directory name says what that purpose is. A module stays a
single file until a second file shares its purpose. Then both move into a
directory named for the purpose, and nothing else moves with them.

Two things follow from that:

- SQL lives only in `db/`. A stage, the search or the interface calls a function
  there. It never writes a query.
- The library knows nothing about GPUI. The user interface lives in `src/ui/`
  and in `src/main.rs`, and it calls into the library. This is what lets the
  whole pipeline be tested without a window.

## Today

    src/
      lib.rs          the library: the data layer, and the window over it
      main.rs         the application. It opens the database and one window
      config.rs       the TOML file, the ignore globs, the bm25 weights
      paths.rs        the data directory, the thumbnail cache, the config file,
                      the speech model
      bin/fb.rs       a headless front end: scan, search, reindex, status
      actions/        what the user does to a file on disk
        clipboard.rs  the Windows clipboard, which GPUI cannot do
        rename.rs     a new name on disk and in the index, never over a file
      media/          what a file is
        mod.rs        the three kinds
        hash.rs       blake3 over the size and both ends
        filename.rs   the split for the index, sanitizing, poor names
      db/             the database and every query against it
        mod.rs        open, pragmas, migrations
        migrations.rs the schema
        files.rs      rows in `files`
        fts.rs        the search index, rebuilt from the tables that own the text.
                      A rule that shapes a column belongs here, not in the stage
                      that fills the table: `fb reindex` is seconds, and reading
                      every file again is not
        tags.rs       tags and `file_tags`
        review.rs     the four Review groups, each file with its reason
        segments.rs   speech segments
        ocr.rs        the on-screen text of a file
      ffmpeg/         every call out to the FFmpeg command line tools
        mod.rs        tool lookup, the shared command, the error text
        frames.rs     the frames OCR reads, spread over the duration
        probe.rs      ffprobe and its JSON
        thumbnail.rs  the `thumbnail` filter
        whisper.rs    the `whisper` filter and its output
      ocr/            the recognizer, behind one interface
        mod.rs        the `Engine` trait and the recognizer of this machine
        windows.rs    `Windows.Media.Ocr` through `windows-rs`
        noise.rs      text many files hold that nobody searches for
      index/          filling the database from the files on disk
        mod.rs
        jobs.rs       the durable queue
        scan.rs       the walk over the roots
        watch.rs      the watcher over the roots and the incremental scan
        pipeline.rs   the worker pool. When a stage runs, and what a failure means
        stages/       what a stage produces
          mod.rs      one arm per stage
          probe.rs
          thumbnail.rs
          whisper.rs  the rules that drop hallucinated text
          ocr.rs      the line filter and the deduplication
      search/         the ranked query
        mod.rs        the query against `fts` and `files`
        query.rs      what the user typed becomes an FTS5 expression
      ui/             the window. The only directory that knows about GPUI
        mod.rs        the view that owns the state, the keys and the layout
        grid.rs       the virtual grid of results
        tile.rs       one result: thumbnail, hover video, badge, matched text
        preview.rs    the side pane and its three tabs
        tagger.rs     the tag input and its autocomplete
        renamer.rs    the rename input in the side pane
        review.rs     the Review group tabs
        pages.rs      the Browse, Review and Settings tabs
        settings.rs   the Settings form
        chips.rs      the media kind filters

`config` and `paths` stay at the root because every directory uses them and they
belong to none.

Unit tests sit in the file they test, because most of them test one function.
`tests/pipeline.rs` is the one integration test. It generates three fixtures with
FFmpeg into a temporary directory, so it needs nothing from the repository.

## Where the rest of v1 goes

| Issue | New code | Where |
| --- | --- | --- |
| 05 file actions | `CF_HDROP` through `clipboard-win`, reveal in Explorer | `src/actions/` |
| 06 tags | The tag input and its autocomplete | `ui/tag_input.rs`. The queries are in `db/tags.rs` already |
| 08 rename | Sanitizing a name the user typed | `media/filename.rs`. The rename itself, which moves the file and updates the row: `src/actions/rename.rs` |
| 09 review screen | The queries for duplicate groups, long files, failures and poor names | `db/review.rs`. The screen and the bulk rename walk: `ui/review.rs` |

## The window

`ui` holds one view, `FileBuddy`, and it owns everything that changes: the
query, the results, the selection, the chips and the two videos that may play.
The other files in `ui` only draw. A free function that takes `&FileBuddy` and
`&mut Context<FileBuddy>` keeps each part in its own file without a second
entity to keep in step. Add a screen the same way.

## Adding a stage

The pipeline was built so that issues 07 and 10 are additions, not changes.

1. Add the variant to `jobs::Stage`, with its `as_str`, its `concurrency` and
   its place in `Stage::after_probe`.
2. Add `index/stages/<name>.rs` with one `run` function.
3. Add the arm to `index::stages::run`.
4. Add the table that holds the result to `db/`, and rebuild the matching `fts`
   column in `db/fts.rs`.

Nothing in `pipeline.rs` changes. It already counts attempts, stops after two
failures, records the error on the file and never retries a file that FFmpeg
cannot open.

## Adding a column to the search

`fts` has four columns and the weights are in the config. A fifth signal is a
schema migration, a fifth weight in `config::Weights`, a fifth column in the
`bm25` call in `search::search`, and a fifth line in `db::fts::refresh`. Keep
the column order the same in all four places, because `bm25` takes the weights
by position.
