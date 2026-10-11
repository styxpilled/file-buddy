//! Files dropped on the window from Explorer, and `forget_file`, which takes the
//! marked files or the selection out of the index.

use std::path::PathBuf;

use anyhow::Result;
use gpui::{Context, Window};

use crate::index::dropped::{self, Added, Forgot};
use crate::index::jobs::Stage;
use crate::ui::{FileBuddy, Page, counted};

impl FileBuddy {
    /// A drop on the window. The new dropped files open on the Folders page.
    pub(super) fn add_dropped(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let roots = self.config.roots.clone();
        let Some(work) = self.index_work(
            move |db, ignore, cache| dropped::add(db, &paths, &roots, ignore, cache),
            cx,
        ) else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let added = work.await;
            this.update_in(cx, |this, window, cx| {
                this.show_dropped(added, window, cx);
                this.index_done(cx);
            })
            .ok();
        })
        .detach();
    }

    fn show_dropped(&mut self, added: Result<Added>, window: &mut Window, cx: &mut Context<Self>) {
        let added = match added {
            Ok(added) => added,
            Err(e) => {
                log::error!("adding the dropped files failed: {e:#}");
                self.flash(format!("adding failed: {e:#}"), true, cx);
                return;
            }
        };
        if added.dropped.is_empty() {
            self.rerun_search(cx);
        } else if self.page() == Page::Folders {
            self.open_dropped(window, cx);
        } else {
            self.record_step(cx);
            self.folder = None;
            self.dropped = true;
            self.end_likes();
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.enter_page(Page::Folders, None, window, cx);
        }
        if let Some(at) = self.results.iter().position(|entry| {
            entry
                .hit()
                .is_some_and(|hit| added.dropped.contains(&hit.file.id))
        }) {
            self.select(at, cx);
        }
        self.flash(drop_summary(&added), false, cx);
        self.index_text(Stage::Ocr, cx);
    }

    /// A dropped file leaves the index. A root file comes back at once, and
    /// every stage runs again.
    pub(super) fn forget_files(&mut self, cx: &mut Context<Self>) {
        let targets = self.delete_targets();
        if targets.is_empty() {
            return;
        }
        for file in &targets {
            self.release_videos(&file.hash);
        }
        let hashes: Vec<String> = targets.iter().map(|file| file.hash.clone()).collect();
        self.update_index(
            move |db, _, cache| dropped::forget(db, &targets, cache),
            move |this, forgot, cx| {
                match forgot {
                    Ok(forgot) => this.flash(forget_summary(&forgot), false, cx),
                    Err(e) => {
                        log::error!("forget failed: {e:#}");
                        this.flash(format!("forget failed: {e:#}"), true, cx);
                    }
                }
                let cache = this.cache.clone();
                this.thumbnails.update(cx, |thumbnails, _| {
                    for hash in &hashes {
                        thumbnails.forget(hash, &cache);
                    }
                });
                this.rerun_search(cx);
                this.reload_review(cx);
                this.schedule_preview(cx);
                this.index_text(Stage::Ocr, cx);
            },
            cx,
        );
    }
}

fn drop_summary(added: &Added) -> String {
    match (added.joined, added.known) {
        (0, 0) => "no media files in the drop".into(),
        (0, known) => format!("{} already in the index", counted(known)),
        (joined, 0) => format!("{} joined", counted(joined)),
        (joined, known) => format!("{} joined, {known} already in the index", counted(joined)),
    }
}

fn forget_summary(forgot: &Forgot) -> String {
    match (forgot.forgotten, forgot.rescanned) {
        (0, rescanned) => format!("scanning {} again", counted(rescanned)),
        (forgotten, 0) => format!("forgot {}", counted(forgotten)),
        (forgotten, rescanned) => {
            format!("forgot {}, scanning {rescanned} again", counted(forgotten))
        }
    }
}
