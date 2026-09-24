# 31 - Back and forward history

Status: ready-for-agent

Depends on nothing. Read the History section of the v2 spec first. If 22 is
done, "more like this" is a step too. If not, add it in 22.

The back and forward buttons of a mouse, and `alt-left` and `alt-right`, move
through a history of where the user has been. There is no history yet.

## Found without testing

GPUI gives `MouseButton::Navigate(NavigationDirection::Back | Forward)`
(`gpui-pre` 0.3.1, `src/interactive.rs`), and the Windows backend maps
XBUTTON1 and XBUTTON2 to it.

## Work

- A step saves the state the user leaves. These make a step:
  - a page switch, by key, by tab, or by escape back to Browse,
  - opening a folder, Backspace, a breadcrumb click,
  - a Review group,
  - "more like this".
- Typing makes no step. The query is part of the saved state, so "type `cat`,
  open a folder, press back" returns to Browse with `cat` typed.
- A step brings back the page, the folder, the query, the Review group, the
  selection and the scroll position. Follow the selection by file, not by row,
  as `update_results` does.
- The chips, the sort and the view are not part of a step.
- A new step after going back drops the forward steps. History lives in memory,
  is lost on close, and holds at most 100 steps. A folder that no longer exists
  is skipped.
- Back and forward close an open tag panel or rename input and navigate at once,
  as a page switch does today. Unsaved Settings changes behave as on a page
  switch.
- The mouse buttons and `alt-left` and `alt-right` are fixed keys. Add them to
  the fixed list in issue 26. By the rule there, `alt-left` and `alt-right`
  work everywhere, the search box included. Check that gpui-component's text
  input binds neither.
- Unit tests: step, back, forward, the dropped forward steps, the limit, and a
  missing folder.

## Comments
