# Source layout

Where code goes, and why. Read this before you add a file to `src/`.

## The rule

A directory is a purpose, not a layer. Files that serve one purpose live
together, and the directory name says what that purpose is. A module stays a
single file until a second file shares its purpose. Then both move into a
directory named for the purpose, and nothing else moves with them.

Two things follow from that:

- SQL lives in `db/`, and in two modules that are each one table's worth of
  queries: `index/jobs.rs`, the queue, and `search/`, the ranked query. A stage
  or the interface calls a function there. It never writes a query.
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
      bin/fb.rs       a headless front end: scan, search, visual, like,
                      reindex, status, and the worker processes
      worker.rs       a model in a child `fb` process, one JSON line each way
      speech.rs       whisper-rs: the model the `whisper` worker loads once
      studio.rs       the Studio edit: the source, the range, the export name
      clip/           visual, as vectors, through ONNX Runtime
        mod.rs        the runtime DLLs, the model config, the vector math
        image.rs      the image encoder, which the `clip` worker loads once
        text.rs       the tokenizer and the text encoder, for search
      actions/        what the user does to a file on disk
        clipboard.rs  the Windows clipboard, which GPUI cannot do
        delete.rs     to the recycle bin, or for good, through the Windows shell
        rename.rs     a new name on disk and in the index, never over a file
      media/          what a file is
        mod.rs        the three kinds
        hash.rs       blake3 over the size and both ends
        filename.rs   the split for the index, sanitizing, poor names
      db/             the database and every query against it
        mod.rs        open, pragmas, migrations
        migrations.rs the schema
        files.rs      rows in `files`, and `read_row`, which every query of
                      files reads with
        fts.rs        the search index, rebuilt from the tables that own the text.
                      A rule that shapes a column belongs here, not in the stage
                      that fills the table: `fb reindex` is seconds, and reading
                      every file again is not
        tags.rs       tags and `file_tags`
        review.rs     the four Review groups, each file with its reason
        segments.rs   speech segments
        edits.rs      the segments and screen lines the user changed, deleted
                      or added. Like tags, nothing can rebuild them
        ocr.rs        the on-screen text of a file, one row per line with its
                      time
        vectors.rs    the CLIP vector of each sampled frame
      ffmpeg/         every call out to the FFmpeg command line tools
        mod.rs        tool lookup, the shared command, the error text
        audio.rs      the first audio track as 16 kHz mono samples
        export.rs     the arguments of a Studio export, and the run with its
                      progress
        frames.rs     the frames OCR reads and CLIP embeds, spread over the
                      duration
        probe.rs      ffprobe and its JSON
        thumbnail.rs  the `thumbnail` filter
      ocr/            the recognizer, behind one interface
        mod.rs        the `Engine` trait and the recognizer of this machine
        windows.rs    `Windows.Media.Ocr` through `windows-rs`
        noise.rs      text many files hold that nobody searches for
      index/          filling the database from the files on disk
        mod.rs
        jobs.rs       the durable queue
        scan.rs       the walk over the roots
        watch.rs      the watcher over the roots and the incremental scan
        pipeline.rs   the thread pool. When a stage runs, and what a failure means
        stages/       what a stage produces
          mod.rs      one arm per stage, the file each job reads, and the
                      worker processes of a stage run
          probe.rs
          whisper.rs  the rules that drop hallucinated text
          ocr.rs      the line filter and the deduplication
          clip.rs
      search/         the ranked query
        mod.rs        the query against `fts` and `files`
        query.rs      what the user typed becomes an FTS5 expression
        folder.rs     one folder of the roots, and why a file in it is not found
        moment.rs     the speech segment or screen line that a match came from
        visual.rs     the vectors in memory, the cutoff, the merge with the
                      text matches, and the search by the look of files
      ui/             the window. The only directory that knows about GPUI
        mod.rs        the view, its actions, the results and the selection
        keys.rs       the default keys, the fixed keys, the `[keys]` changes
                      and the rules that join them
        header.rs     the page tabs, the search box, the notices and the window
                      buttons. It is the title bar. Also the bar over the
                      results: chips, view buttons, count
        grid.rs       the virtual grid of results
        history.rs    back and forward: the steps, and the state each one
                      brings back
        tile.rs       one result: thumbnail, hover video, badge, matched text
        table.rs      the table view of Browse, Folders and Review
        when.rs       local dates, and how long ago a file changed
        preview.rs    the side pane and its two tabs
        rows.rs       the cursor and the input that edit the Speech and Text
                      tabs
        seek_bar.rs   the bar under the clip in the pane and in the Studio, the
                      range handles, and the skip keys
        player.rs     the clip in the pane, the file opened ahead, the tile
                      that plays, and the sound
        indexing.rs   the scan, the watcher and the text stages while the
                      window is open
        tagger.rs     the tag input and its autocomplete
        thumbnails.rs the decoded thumbnails, at most 200, the least recently
                      shown dropped first
        renamer.rs    the rename input in the side pane
        delete.rs     `delete` and `shift-delete` on the marked files or the
                      selection
        folders.rs    the Folders page: the folder, the breadcrumb, the sort
        review.rs     the Review page, its groups and the rename walk
        settings.rs   the Settings page
        studio.rs     the Studio page: the preview that loops the range, the
                      panel, and the export while it runs
        visual.rs     the visual half of a search, which merges in after the
                      text matches, and `a` with its escape steps
        sim.rs        simulated input for the dev window, behind the `sim`
                      feature: a named pipe of `drive.ps1` steps

`config` and `paths` stay at the root because every directory uses them and they
belong to none.

Unit tests sit in the file they test, because most of them test one function.
`tests/pipeline.rs` is the one integration test. It generates three fixtures with
FFmpeg into a temporary directory, so it needs nothing from the repository.

## The window

`ui` holds one view, `FileBuddy`, and it owns everything that changes. Each file
beside `mod.rs` is one part of the window: an `impl FileBuddy` block with the
methods that change that part, and the free functions that draw it. A draw
function takes `&FileBuddy` and `&mut Context<FileBuddy>`, so each part stays in
its own file without a second entity to keep in step. A child module can read
the private fields of `FileBuddy`, so no field is public for this.

State that only one part uses is a struct in that part's file, held in one field
of `FileBuddy`: `Player`, `Indexer`, `Tagger`, `Renamer`. State that only one
page has is in `Mode`, so Review and Settings cannot both be open.

New results go through `show_results`, which starts over at the top, or
`update_results`, which keeps the selection on the same file. Do not set
`results` anywhere else: the selection, the marks and the videos all point into
it.

Every button that acts on a click takes `.cursor_pointer()`. `Button` puts its
own arrow first and the caller's style over it. A disabled button keeps the
arrow.

## Adding a stage

1. Add the variant to `jobs::Stage`, with its `as_str`, its `concurrency` and
   its place in `Stage::after_probe`.
2. Add the arm to `index::stages::run`. A stage that does more than call
   FFmpeg gets `index/stages/<name>.rs` with one `run` function, which takes
   the row of a file that holds the bytes.
3. Add the table that holds the result to `db/`, and rebuild the matching `fts`
   column in `db/fts.rs`.

Nothing in `pipeline.rs` changes. It already counts attempts, stops after two
failures, records the error on the file and never retries a file that FFmpeg
cannot open.

## Adding a column to the search

`fts` has four columns and the weights are in the config. A fifth signal is a
schema migration, a fifth weight in `config::Weights`, a fifth column in the
`bm25` call in `search::search` and in `Signal::COLUMNS`, and a fifth line in
`db::fts::refresh`. Keep the column order the same in all of them, because
`bm25` takes the weights by position.
