//! Files dropped on the window from Explorer or pasted, and `forget_file`,
//! which takes the marked files or the selection out of the index.

use std::path::PathBuf;

use anyhow::Result;
use gpui::{Context, Window};

use crate::actions::clipboard::{self, Pasted};
use crate::db::files;
use crate::index::dropped::{self, Added, Forgot};
use crate::index::jobs::Stage;
use crate::studio;
use crate::ui::{FileBuddy, Page, counted, when};

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

    /// `ctrl-v` outside the inputs. Files join as a drop does. An image goes
    /// to the export folder, which is a root, and is selected when the watcher
    /// indexes it.
    pub(super) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match clipboard::paste() {
            Ok(Pasted::Files(paths)) => self.add_dropped(paths, window, cx),
            Ok(Pasted::Image(png)) => self.save_pasted(&png, cx),
            Ok(Pasted::Nothing) => self.flash("no files or image to paste", true, cx),
            Err(e) => {
                log::error!("paste failed: {e:#}");
                self.flash(format!("paste failed: {e}"), true, cx);
            }
        }
    }

    fn save_pasted(&mut self, png: &[u8], cx: &mut Context<Self>) {
        let Some(folder) = self.config.export_folder.clone() else {
            self.flash("set an export folder in Settings", true, cx);
            return;
        };
        let name = format!("pasted {}.png", when::file_stamp_now());
        let path = studio::unique_path(&folder, &name);
        if let Err(e) = std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&path, png)) {
            log::error!("writing {} failed: {e}", path.display());
            self.flash(format!("paste failed: {e}"), true, cx);
            return;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        self.flash(format!("pasted {name}"), false, cx);
        self.pasted = Some(dropped::place(&path, &self.config.roots).0);
    }

    /// After the watcher indexed a batch: the pasted image, once it is in the
    /// index, is selected if the results show it.
    pub(super) fn select_pasted(&mut self, cx: &mut Context<Self>) {
        let Some(path) = &self.pasted else {
            return;
        };
        let Ok(Some(file)) = files::by_path(self.db.conn(), path) else {
            return;
        };
        self.pasted = None;
        if let Some(at) = self
            .results
            .iter()
            .position(|entry| entry.hit().is_some_and(|hit| hit.file.id == file.id))
        {
            self.select(at, cx);
        }
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
