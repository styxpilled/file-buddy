//! The Folders page: one folder of the roots at a time, the breadcrumb over it
//! and the sort of its files. The search box searches the folder and every
//! folder under it.

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, Window};
use gpui_kit::component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_kit::component::{Sizable, h_flex};

use crate::db::files::FileRow;
use crate::search::folder::{self as listing, Entry, Sort};
use crate::ui::{FileBuddy, Page, counted, toggle};

pub(super) const BROWSE_PLACEHOLDER: &str = "search database";

impl FileBuddy {
    /// The contents of the folder the Folders page shows, or the roots.
    pub(super) fn list_folder(&mut self) -> Vec<Entry> {
        // The folder can be deleted, or its root taken out of the settings,
        // while the page shows it.
        while let Some(dir) = &self.folder
            && !(dir.is_dir() && self.config.roots.iter().any(|root| dir.starts_with(root)))
        {
            self.folder = self.parent_folder(dir);
        }
        let Some(dir) = &self.folder else {
            self.status = folders_counted(self.config.roots.len()).into();
            return self
                .config
                .roots
                .iter()
                .cloned()
                .map(Entry::Folder)
                .collect();
        };
        let listed = self.config.ignore_set().and_then(|ignore| {
            listing::list(self.db.conn(), dir, &ignore, &self.kinds, self.folder_sort)
        });
        match listed {
            Ok(entries) => {
                let folders = entries
                    .iter()
                    .filter(|entry| matches!(entry, Entry::Folder(_)))
                    .count();
                let files = entries.len() - folders;
                self.status = match folders {
                    0 => counted(files).into(),
                    n => format!("{}, {}", folders_counted(n), counted(files)).into(),
                };
                entries
            }
            Err(e) => {
                log::error!("listing {} failed: {e:#}", dir.display());
                self.status = format!("cannot read the folder: {e:#}").into();
                Vec::new()
            }
        }
    }

    /// One level up, or `None` from a root. Nothing above a root is shown.
    fn parent_folder(&self, dir: &Path) -> Option<PathBuf> {
        let roots = &self.config.roots;
        if roots.iter().any(|root| root == dir) {
            return None;
        }
        dir.parent()
            .filter(|parent| roots.iter().any(|root| parent.starts_with(root)))
            .map(Path::to_path_buf)
    }

    /// Show another folder. A query searches the folder it was typed in, so
    /// it goes when the folder changes.
    pub(super) fn open_folder(
        &mut self,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let left = std::mem::replace(&mut self.folder, folder);
        self.end_likes();
        if !self.input.read(cx).value().is_empty() {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.run_search(cx);
        // Up a level, the folder the user came out of stays selected.
        if let Some(left) = left
            && let Some(at) = self
                .results
                .iter()
                .position(|entry| matches!(entry, Entry::Folder(path) if *path == left))
        {
            self.select(at, cx);
        }
    }

    /// Backspace on the Folders page.
    pub(super) fn go_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page() != Page::Folders {
            return;
        }
        if let Some(dir) = &self.folder {
            let parent = self.parent_folder(dir);
            self.open_folder(parent, window, cx);
        }
    }

    /// The sort of the page. Browse and Folders each keep their own.
    pub(super) fn sort(&self) -> Sort {
        match self.page() {
            Page::Folders => self.folder_sort,
            _ => self.browse_sort,
        }
    }

    /// `s`, or a sort button. The selection stays on the same entry.
    pub(super) fn set_sort(&mut self, sort: Sort, window: &mut Window, cx: &mut Context<Self>) {
        let kept = match self.page() {
            Page::Folders => &mut self.folder_sort,
            Page::Browse => &mut self.browse_sort,
            Page::Review | Page::Settings => return,
        };
        if *kept == sort {
            return;
        }
        *kept = sort;
        self.rerun_search(cx);
        self.reveal_selection(window, cx);
    }

    /// On the Folders page the search box says where it searches.
    pub(super) fn sync_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted: SharedString = match (self.page(), &self.folder) {
            (Page::Folders, None) => "search in directories".into(),
            (Page::Folders, Some(dir)) => display(dir).into(),
            _ => BROWSE_PLACEHOLDER.into(),
        };
        if self.placeholder != wanted {
            self.placeholder = wanted.clone();
            self.input
                .update(cx, |input, cx| input.set_placeholder(wanted, window, cx));
        }
    }

    /// What a tile or a row calls a folder. The list of roots names each one
    /// in full.
    pub(super) fn folder_name(&self, path: &Path) -> String {
        match self.folder {
            None => display(path),
            Some(_) => FileRow::name_of(path),
        }
    }
}

pub fn breadcrumb(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let mut stops: Vec<(String, Option<PathBuf>)> = vec![("Folders".into(), None)];
    if let Some(dir) = &this.folder
        && let Some(root) = this.config.roots.iter().find(|root| dir.starts_with(root))
    {
        stops.push((display(root), Some(root.clone())));
        let mut at = root.clone();
        for part in dir
            .strip_prefix(root)
            .into_iter()
            .flat_map(Path::components)
        {
            at.push(part);
            let name = part.as_os_str().to_string_lossy().into_owned();
            stops.push((name, Some(at.clone())));
        }
    }

    let view = cx.entity();
    Breadcrumb::new()
        .children(stops.into_iter().map(|(label, folder)| {
            let view = view.clone();
            BreadcrumbItem::new(label).on_click(move |_, window, cx| {
                let folder = folder.clone();
                view.update(cx, |this, cx| this.open_folder(folder, window, cx));
            })
        }))
        .into_any_element()
}

/// The sort only orders a folder or Browse with nothing typed. A query keeps
/// its ranking, so the sort buttons go while one is typed.
pub fn sort_buttons(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .gap_1()
        .when(this.listing, |buttons| {
            buttons.children(Sort::ALL.map(|sort| {
                toggle(sort.label(), sort.label(), this.sort() == sort)
                    .xsmall()
                    .tooltip("S")
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.set_sort(sort, window, cx)),
                    )
            }))
        })
        .into_any_element()
}

/// A path the way Explorer writes it. A root from the config can mix the two
/// separators.
pub fn display(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}

fn folders_counted(folders: usize) -> String {
    match folders {
        1 => "1 folder".to_string(),
        n => format!("{n} folders"),
    }
}
