//! Editing the Speech and Text tabs. `e` puts a cursor on the rows of the tab.
//! Enter edits the row under it, `delete` deletes it, and `l` adds a row at the
//! time of the clip. A change reaches the index at once.
//!
//! An edit stops the stage from running again for the file, so a tab can be
//! edited only after its stage has run.

use gpui::prelude::*;
use gpui::{Context, Entity, FocusHandle, Focusable, ScrollHandle, Subscription, Window};
use gpui_kit::component::input::{InputEvent, InputState};

use crate::db::edits::{self, Row, Table};
use crate::index::jobs::{self, Stage};
use crate::media::Kind;
use crate::ui::{Detail, FileBuddy};

pub(super) struct RowEditor {
    /// The file the rows belong to.
    pub(super) hash: String,
    pub(super) table: Table,
    pub(super) cursor: usize,
    pub(super) focus: FocusHandle,
    pub(super) scroll: ScrollHandle,
    pub(super) input: Option<RowInput>,
    /// Focus that leaves the rows and their input ends the editing, and drops
    /// what was typed.
    _focus_out: Subscription,
}

pub(super) struct RowInput {
    pub(super) state: Entity<InputState>,
    pub(super) target: Target,
    _subscription: Subscription,
}

pub(super) enum Target {
    /// The row as it was read.
    Edit(Row),
    /// A new row after the cursor, at this time.
    Add(Option<i64>),
}

/// The table a tab edits.
pub(super) fn table_of(detail: Detail) -> Table {
    match detail {
        Detail::Speech => Table::Segments,
        Detail::Text => Table::Ocr,
    }
}

impl FileBuddy {
    /// `e`, or a click on a row with its index. The cursor starts on the row
    /// that the clip is at.
    pub(super) fn edit_rows(
        &mut self,
        at: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let table = table_of(self.shown_detail());
        let Some(hit) = self.selected_hit() else {
            return;
        };
        if table == Table::Segments && hit.file.kind != Kind::Video {
            return;
        }
        let hash = hit.file.hash.clone();
        let stage = match table {
            Table::Segments => Stage::Whisper,
            Table::Ocr => Stage::Ocr,
        };
        let state = jobs::state(self.db.conn(), &hash, stage).ok().flatten();
        if !state.is_some_and(|(state, _)| matches!(state.as_str(), "done" | "failed" | "skipped"))
        {
            let why = match table {
                Table::Segments => "the speech is not transcribed yet",
                Table::Ocr => "the text is not read yet",
            };
            self.flash(why, true, cx);
            return;
        }
        let rows = self.rows_of(table, &hash);
        let cursor = at.unwrap_or_else(|| {
            let now = self.clip_ms(&hash).unwrap_or(0);
            rows.iter()
                .rposition(|row| row.time_ms.is_some_and(|time| time <= now))
                .unwrap_or(0)
        });

        let editor = match self.rows.take() {
            Some(editor) if editor.hash == hash && editor.table == table => editor,
            _ => {
                let focus = cx.focus_handle();
                let focus_out = cx.on_focus_out(&focus, window, |this, _, _, cx| {
                    this.rows = None;
                    cx.notify();
                });
                RowEditor {
                    hash,
                    table,
                    cursor: 0,
                    focus,
                    scroll: ScrollHandle::new(),
                    input: None,
                    _focus_out: focus_out,
                }
            }
        };
        let editor = self.rows.insert(RowEditor { cursor, ..editor });
        editor.scroll.scroll_to_item(cursor);
        window.focus(&editor.focus, cx);
        cx.notify();
    }

    /// Up and down in the rows. The clip follows the cursor, so the row can be
    /// heard or seen while it is read.
    pub(super) fn move_row(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(editor) = &self.rows else {
            return;
        };
        let rows = self.rows_of(editor.table, &editor.hash);
        let Some(last) = rows.len().checked_sub(1) else {
            return;
        };
        let cursor = if down {
            (editor.cursor + 1).min(last)
        } else {
            editor.cursor.saturating_sub(1).min(last)
        };
        if let Some(editor) = &mut self.rows {
            editor.cursor = cursor;
            editor.scroll.scroll_to_item(cursor);
        }
        if let Some(time) = rows[cursor].time_ms {
            self.seek_to(time, cx);
        }
        cx.notify();
    }

    /// Enter. The row under the cursor becomes an input, with its text selected.
    /// With no rows, it adds one.
    pub(super) fn edit_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.rows else {
            return;
        };
        let Some(row) = self
            .rows_of(editor.table, &editor.hash)
            .into_iter()
            .nth(editor.cursor)
        else {
            self.add_row(window, cx);
            return;
        };
        let text = row.text.clone();
        self.open_row_input(Target::Edit(row), &text, window, cx);
    }

    /// `l`. An empty input after the cursor, for a row at the time of the clip.
    pub(super) fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.rows else {
            return;
        };
        let still = self
            .selected_hit()
            .is_some_and(|hit| hit.file.kind == Kind::Still);
        // A still has one frame, so a time in it says nothing.
        let time = (!still).then(|| self.clip_ms(&editor.hash).unwrap_or(0));
        if let Some(editor) = &mut self.rows {
            editor.scroll.scroll_to_item(editor.cursor + 1);
        }
        self.open_row_input(Target::Add(time), "", window, cx);
    }

    /// `delete`. The row stays in the table with no text, so a later run of the
    /// stage cannot bring it back.
    pub(super) fn delete_row(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.rows else {
            return;
        };
        let (table, hash) = (editor.table, editor.hash.clone());
        let Some(row) = self.rows_of(table, &hash).into_iter().nth(editor.cursor) else {
            return;
        };
        let result = edits::delete(self.db.conn(), table, &hash, &row);
        self.after_row_change(result, None, cx);
    }

    fn open_row_input(
        &mut self,
        target: Target,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = cx.new(|cx| InputState::new(window, cx).default_value(text.to_string()));
        state.update(cx, |state, cx| state.select_all(window, cx));
        let subscription =
            cx.subscribe_in(&state, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.save_row(window, cx);
                }
            });
        let handle = state.read(cx).focus_handle(cx);
        if let Some(editor) = &mut self.rows {
            editor.input = Some(RowInput {
                state,
                target,
                _subscription: subscription,
            });
        }
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Enter in the input. Empty text deletes the row.
    fn save_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.rows else {
            return;
        };
        let Some(input) = editor.input.take() else {
            return;
        };
        let typed = input.state.read(cx).value().trim().to_string();
        let (table, hash) = (editor.table, editor.hash.clone());
        window.focus(&editor.focus, cx);

        let conn = self.db.conn();
        match input.target {
            Target::Edit(row) if row.text == typed => cx.notify(),
            Target::Edit(row) => {
                let result = edits::set_text(conn, table, &hash, &row, &typed);
                self.after_row_change(result, None, cx);
            }
            Target::Add(_) if typed.is_empty() => cx.notify(),
            Target::Add(time) => {
                let result = edits::add(conn, table, &hash, time, &typed);
                let id = result.as_ref().ok().copied();
                self.after_row_change(result.map(|_| ()), id, cx);
            }
        }
    }

    /// Show the change in the tab and the results. The cursor goes to the row
    /// just added, or stays in place.
    fn after_row_change(
        &mut self,
        result: anyhow::Result<()>,
        added: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = result {
            log::error!("editing a row failed: {e:#}");
            self.flash(format!("edit failed: {e}"), true, cx);
        }
        if let Some(editor) = &self.rows {
            let rows = self.rows_of(editor.table, &editor.hash);
            let cursor = added
                .and_then(|id| rows.iter().position(|row| row.id == id))
                .unwrap_or(editor.cursor)
                .min(rows.len().saturating_sub(1));
            if let Some(editor) = &mut self.rows {
                editor.cursor = cursor;
                editor.scroll.scroll_to_item(cursor);
            }
        }
        self.rerun_search(cx);
    }

    /// Escape in the input. The rows keep the cursor.
    pub(super) fn close_row_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.rows {
            editor.input = None;
            window.focus(&editor.focus, cx);
        }
        cx.notify();
    }

    pub(super) fn close_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rows = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The cursor belongs to the rows of one tab of one file. Another file,
    /// another tab, or the tag or rename input ends it.
    pub(super) fn sync_rows(&mut self) {
        let Some(editor) = &self.rows else {
            return;
        };
        let same_file = self
            .selected_hit()
            .is_some_and(|hit| hit.file.hash == editor.hash);
        if !same_file
            || table_of(self.shown_detail()) != editor.table
            || self.tagger.is_some()
            || self.renamer.is_some()
        {
            self.rows = None;
        }
    }

    fn rows_of(&self, table: Table, hash: &str) -> Vec<Row> {
        edits::rows(self.db.conn(), table, hash).unwrap_or_else(|e| {
            log::error!("reading the rows failed: {e:#}");
            Vec::new()
        })
    }

    /// Where the clip of this file is, if it plays in the pane.
    fn clip_ms(&self, hash: &str) -> Option<i64> {
        self.player
            .video_of(hash)
            .map(|video| video.position().as_millis() as i64)
    }
}
