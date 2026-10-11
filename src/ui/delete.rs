//! `delete` and `shift-delete`: the marked files, or the selection, to the
//! recycle bin, or gone for good after the shell asks. An empty name in the
//! rename input sends that file to the recycle bin too.

use gpui::prelude::*;
use gpui::{Context, Window};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::actions::delete::{self, Mode};
use crate::db::files::{self, FileRow};
use crate::search::folder::Entry;
use crate::ui::{FileBuddy, counted};

impl FileBuddy {
    /// The files a delete acts on: every marked result, as the grid shows them,
    /// or the selection.
    pub(super) fn delete_targets(&self) -> Vec<FileRow> {
        if self.marked.is_empty() {
            return self
                .selected_hit()
                .map(|hit| vec![hit.file.clone()])
                .unwrap_or_default();
        }
        self.results
            .iter()
            .filter_map(Entry::hit)
            .filter(|hit| self.is_marked(hit))
            .map(|hit| hit.file.clone())
            .collect()
    }

    pub(super) fn delete_files(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.delete_targets();
        self.delete(targets, mode, false, window, cx);
    }

    /// `walk`: the delete came from the rename walk, which goes on to the next
    /// file when it is done.
    pub(super) fn delete(
        &mut self,
        targets: Vec<FileRow>,
        mode: Mode,
        walk: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.deleting.is_empty() || targets.is_empty() {
            return;
        }
        // The pointer can rest on a tile, and a hover opens the clip again the
        // moment it closes, so the gate goes up first.
        self.deleting = targets.iter().map(|file| file.hash.clone()).collect();
        for file in &targets {
            self.release_videos(&file.hash);
        }
        cx.notify();
        // SAFETY: no arguments. The key or the click came to this window, so
        // it is the one in front.
        let owner = unsafe { GetForegroundWindow() }.0 as isize;
        let paths = targets.iter().map(|file| file.path.clone()).collect();
        let work = cx.background_spawn(async move { delete::delete(paths, mode, owner) });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            this.update_in(cx, |this, window, cx| {
                this.finish_delete(&targets, mode, result, walk, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// The user can cancel, and one file can fail while the others go, so what
    /// counts is which files are no longer on disk.
    fn finish_delete(
        &mut self,
        targets: &[FileRow],
        mode: Mode,
        result: anyhow::Result<()>,
        walk: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.deleting.clear();
        let gone: Vec<&FileRow> = targets.iter().filter(|file| !file.path.exists()).collect();
        for file in &gone {
            if let Err(e) = files::mark_gone(self.db.conn(), file.id) {
                log::error!(
                    "removing {} from the index failed: {e:#}",
                    file.path.display()
                );
            }
        }
        match result {
            Err(e) => {
                log::error!("delete failed: {e:#}");
                self.flash(format!("{e:#}"), true, cx);
            }
            Ok(()) if gone.is_empty() => {}
            Ok(()) => {
                let files = counted(gone.len());
                let text = match mode {
                    Mode::Trash => format!("moved {files} to the recycle bin"),
                    Mode::Delete => format!("deleted {files}"),
                };
                self.flash(text, false, cx);
            }
        }
        if walk {
            self.walk_past(&gone, window, cx);
        } else if !gone.is_empty() {
            self.rerun_search(cx);
            self.reload_review(cx);
        }
        self.schedule_preview(cx);
    }

    /// The walk keeps its results, because a named file must not leave the list
    /// under the user, so only the file that went leaves it. The next file then
    /// takes its place under the input.
    fn walk_past(&mut self, gone: &[&FileRow], window: &mut Window, cx: &mut Context<Self>) {
        if gone.is_empty() {
            // Cancelled or failed: the name is still to be given.
            self.open_renamer(window, cx);
            return;
        }
        let at = self.selected;
        let kept = self
            .results
            .iter()
            .filter(|entry| {
                entry
                    .hit()
                    .is_none_or(|hit| gone.iter().all(|file| file.id != hit.file.id))
            })
            .cloned()
            .collect();
        self.update_results(kept, cx);
        self.count_groups(cx);
        if at < self.results.len() {
            self.select(at, cx);
            self.open_renamer(window, cx);
        } else {
            self.flash("the walk is done", false, cx);
        }
    }
}
