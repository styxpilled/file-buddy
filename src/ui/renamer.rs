//! The rename input. It takes the place of the file name in the side pane, so
//! the name is edited where it is read. The extension shows beside the input
//! and is not part of it.

use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Focusable, SharedString, Subscription, Task, Window, div};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};

use crate::actions::delete::Mode;
use crate::actions::rename;
use crate::db::files::FileRow;
use crate::search::Hit;
use crate::search::folder::Entry;
use crate::ui::FileBuddy;

/// A decoder closes its file on its own thread, a moment after the clip is
/// dropped, and the rename fails until it has. `RENAME_TRIES * RENAME_RETRY`
/// is how long the rename waits for it.
const RENAME_RETRY: Duration = Duration::from_millis(50);
const RENAME_TRIES: usize = 20;

pub struct Renamer {
    input: Entity<InputState>,
    /// The file as it was when the input opened.
    file: FileRow,
    extension: SharedString,
    /// The rename on its way to disk. A second Enter must not start another.
    saving: Option<Task<()>>,
    _subscription: Subscription,
}

impl FileBuddy {
    /// Open the rename input on the selection, prefilled with its stem and all
    /// of it selected, so typing replaces the name and an arrow key edits it.
    pub(super) fn open_renamer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.selected_hit() else {
            return;
        };
        if self
            .renamer
            .as_ref()
            .is_some_and(|renamer| renamer.file.id == hit.file.id)
        {
            return;
        }
        let file = hit.file.clone();
        self.tagger = None;

        let input =
            cx.new(|cx| InputState::new(window, cx).default_value(rename::stem(&file.path)));
        input.update(cx, |input, cx| input.select_all(window, cx));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.save_rename(window, cx);
                }
            });

        let handle = input.read(cx).focus_handle(cx);
        self.renamer = Some(Renamer {
            input,
            extension: file
                .path
                .extension()
                .map(|extension| format!(".{}", extension.to_string_lossy()).into())
                .unwrap_or_default(),
            file,
            saving: None,
            _subscription: subscription,
        });
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Escape, or a click on another tile outside the walk.
    pub(super) fn close_renamer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renamer = None;
        window.focus(&self.focus, cx);
        // A save that was cut short released the clip.
        self.schedule_preview(cx);
        cx.notify();
    }

    /// The renamer belongs to one file. A click on another tile takes the walk
    /// to that file, and closes a single rename.
    pub(super) fn renamer_follows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renamer) = &self.renamer else {
            return;
        };
        if self
            .selected_hit()
            .is_some_and(|hit| hit.file.id == renamer.file.id)
        {
            return;
        }
        if self.walking() {
            self.open_renamer(window, cx);
        } else {
            self.close_renamer(window, cx);
        }
    }

    fn save_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renamer) = &self.renamer else {
            return;
        };
        if renamer.saving.is_some() {
            return;
        }
        let typed = renamer.input.read(cx).value().to_string();
        let file = renamer.file.clone();
        if typed.trim().is_empty() {
            // No name at all: the file goes to the recycle bin.
            let walk = self.walking();
            self.renamer = None;
            window.focus(&self.focus, cx);
            self.delete(vec![file], Mode::Trash, walk, window, cx);
            return;
        }
        self.release_videos(&file.hash);
        cx.notify();

        let saving = cx.spawn_in(window, async move |this, cx| {
            let mut tries = 0;
            loop {
                let Ok(result) = this.update(cx, |this, _| {
                    rename::rename(this.db.conn_mut(), &file, &typed)
                }) else {
                    return;
                };
                tries += 1;
                match result {
                    Err(e) if rename::is_in_use(&e) && tries < RENAME_TRIES => {
                        cx.background_executor().timer(RENAME_RETRY).await;
                    }
                    result => {
                        this.update_in(cx, |this, window, cx| {
                            this.finish_rename(&file, result, window, cx)
                        })
                        .ok();
                        return;
                    }
                }
            }
        });
        if let Some(renamer) = &mut self.renamer {
            renamer.saving = Some(saving);
        }
    }

    fn finish_rename(
        &mut self,
        file: &FileRow,
        result: anyhow::Result<Option<PathBuf>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = match result {
            Ok(target) => target,
            Err(e) => {
                log::error!("rename of {} failed: {e:#}", file.path.display());
                if let Some(renamer) = &mut self.renamer {
                    renamer.saving = None;
                }
                self.flash(e.to_string(), true, cx);
                self.schedule_preview(cx);
                return;
            }
        };
        if let Some(target) = &target {
            let name = FileRow::name_of(target);
            self.flash(format!("renamed to {name}"), false, cx);
        }

        if !self.walking() {
            self.renamer = None;
            window.focus(&self.focus, cx);
            if self.review().is_some() {
                self.reload_review(cx);
            } else {
                self.rerun_search(cx);
            }
            return;
        }
        self.count_groups(cx);
        if let (Some(target), Some(hit)) = (
            target,
            self.results
                .iter_mut()
                .filter_map(Entry::hit_mut)
                .find(|hit| hit.file.id == file.id),
        ) {
            hit.file.filename = FileRow::name_of(&target);
            hit.file.path = target;
        }
        // A click may have taken the walk to another file while this one saved.
        if self
            .selected_hit()
            .is_some_and(|hit| hit.file.id == file.id)
        {
            self.walk_on(window, cx);
        }
    }

    /// The next file of the walk, or the end of it.
    fn walk_on(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.selected + 1;
        if next < self.results.len() {
            self.select(next, cx);
            self.open_renamer(window, cx);
        } else {
            self.close_renamer(window, cx);
            self.flash("the walk is done", false, cx);
        }
    }
}

/// The input that takes the place of the name in the side pane, while this
/// file is being renamed.
pub fn name_input(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> Option<AnyElement> {
    let renamer = this
        .renamer
        .as_ref()
        .filter(|renamer| renamer.file.id == hit.file.id)?;

    let line = h_flex()
        .w_full()
        .gap_1()
        .items_center()
        .child(div().flex_1().child(Input::new(&renamer.input).small()))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(renamer.extension.clone()),
        );
    Some(line.into_any_element())
}
