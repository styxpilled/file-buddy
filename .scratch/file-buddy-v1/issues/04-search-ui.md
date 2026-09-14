# 04 - Search interface

Status: ready-for-agent

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
