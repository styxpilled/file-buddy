# 04 - Search interface

Status: done

Depends on 02 and 03.

## Work

- `Input` from gpui-component as the search box. It holds focus at all times.
  Any typed character goes to it.
- `VirtualList` for the result grid. A plain container cannot render a few
  thousand tiles.
- `ResizablePanel` for the side pane. It plays the selection and shows the
  transcript.
- Per keystroke search. Each token becomes a prefix term (`tok*`). Rank with
  `bm25` and the weights from the configuration file.
- Arrow keys move the selection. Escape clears the query. An empty query shows
  every file, newest first.
- Hover starts one live looping muted video in the tile. Only one plays at once.
  Still images do not react to hover.
- Each result shows the matched text and a badge for the signal that matched:
  tag, filename, speech or screen text. This is also the only way to judge the
  ranking weights while using the application.
- Chips filter by media kind. All on by default.

## Done when

Typing finds files by name across the whole collection with no visible delay,
and the user can reach any result with the keyboard alone.

## Comments

### 2026-09-14, agent

Done. `cargo test` passes: 31 unit tests and 7 integration tests. `cargo clippy
--all-targets` is clean.

Driven with real keystrokes by `scripts/drive.ps1`, over `test_videos/`, 21
clips: the grid fills, the
thumbnails load, hover plays a tile, `sopr` narrows to one result per keystroke,
Escape brings the whole collection back, and the arrow keys walk the grid and
scroll it, with the side pane playing whatever is selected.

Two faults that only the running window showed:

- **Focus was lost.** Focus taken in the constructor does not survive the window
  opening, so nothing typed reached the search box. Every frame now takes focus
  back, which is also what "the search box always holds focus" asks for.
- **Escape cleared the text but not the results.** `set_value` is not typing, so
  it raises no change event. `clear_query` runs the search itself.

`src/main.rs` is now the application. The issue 02 prototype is gone; the player
it proved is what the tiles and the side pane use.

| File | What it draws |
| --- | --- |
| `ui/mod.rs` | The one view that owns the state, the key bindings and the layout |
| `ui/grid.rs` | The virtual grid |
| `ui/tile.rs` | One result |
| `ui/preview.rs` | The side pane |
| `ui/chips.rs` | The media kind filters |

### Decisions

**The arrow keys beat the caret.** The search box holds focus, and the `Input`
context already binds all four arrows to caret movement. A binding added later
at the same depth wins, so `ui::bind_keys` rebinds them after
`gpui_kit::init`. The cost is real: left and right no longer move the caret
inside the query. The spec asks for arrow keys on the grid, and Escape clears
the whole query, so this is the trade it asks for.

**The side pane waits 180 ms** before it opens a decoder for the selection. A
held arrow key would otherwise open one file per keystroke. The tile under the
pointer opens its decoder at once, because a hover is already a deliberate act.

**`search::search` now takes a `Request`** with the text, the kinds the chips
allow and a limit. The chips become `kind IN (...)`, and three kinds selected is
the same as no filter. The badge comes from four extra `bm25` calls, one per
column, and the matched text from `snippet()` of the column that won.

**The window scans at startup**, in the background, then reruns the search.
Without it the grid shows whatever the last `fb scan` indexed. Issue 11 replaces
this with a watcher.

**The side pane shows the whole clip, and can be unmuted and paused.** The
video element letterboxes the frame in the size it is given, but only when it is
given both a width and a height: with one of them the element takes the size of
the video itself and the box clips it. The stage now takes the pane width and
whatever height the window leaves, and passes both. Sound and pause are kept
across selections, so the sound does not come back by itself when the selection
moves. A tile stays silent whatever the buttons say.

`scripts/drive.ps1` came out of this. It types into the running window and takes
a picture of it. `docs/agents/checking-the-window.md` says why it needs the
`AttachThreadInput` dance and why it covers the desktop with the terminal first.
It also clicks, which is how the two pane buttons were checked.

### Not done here

Enter does nothing yet. It copies the file in issue 05. Speech and screen text
have no rows yet, so every badge today says `filename`.
