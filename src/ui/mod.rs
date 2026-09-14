//! The window. One view owns the state: the query, the results, the selection
//! and the two videos that may play. The files beside this one only draw it.

mod chips;
mod grid;
mod preview;
mod tile;

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, Focusable, KeyBinding, ScrollStrategy, SharedString, Task, Window, div,
    px,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme, ResizableState, VirtualListScrollHandle, h_flex, h_resizable, resizable_panel,
    v_flex,
};
use gpui_video::Video;
use url::Url;

use crate::config::{Config, Weights};
use crate::db::Db;
use crate::index;
use crate::media::Kind;
use crate::search::{self, Hit};

/// How long the selection must rest before the side pane opens a decoder. Held
/// arrow keys would otherwise open one file per keystroke.
const PREVIEW_DELAY: Duration = Duration::from_millis(180);
pub const SIDE_PANE_WIDTH: f32 = 380.;

gpui_kit::actions!(
    file_buddy,
    [SelectLeft, SelectRight, SelectUp, SelectDown, ClearQuery]
);

/// The search box owns the keyboard, so the grid keys are bound in its context.
/// A binding added later wins over one added before it at the same depth, which
/// is how the arrow keys reach the grid instead of the caret.
pub fn bind_keys(cx: &mut App) {
    for context in [Some("Input"), None] {
        cx.bind_keys([
            KeyBinding::new("left", SelectLeft, context),
            KeyBinding::new("right", SelectRight, context),
            KeyBinding::new("up", SelectUp, context),
            KeyBinding::new("down", SelectDown, context),
            KeyBinding::new("escape", ClearQuery, context),
        ]);
    }
}

pub struct FileBuddy {
    db: Db,
    weights: Weights,
    cache: PathBuf,
    input: Entity<InputState>,
    panes: Entity<ResizableState>,
    scroll: VirtualListScrollHandle,
    results: Vec<Hit>,
    selected: usize,
    /// The chips that are on. All three by default.
    kinds: Vec<Kind>,
    /// Tiles across the grid, from the width of the last frame.
    columns: usize,
    /// The one tile that plays. Only one does.
    hovered: Option<(usize, Video)>,
    preview: Option<Video>,
    preview_task: Option<Task<()>>,
    /// Kept across selections, so the sound does not come back by itself.
    preview_muted: bool,
    preview_paused: bool,
    status: SharedString,
}

impl FileBuddy {
    pub fn new(
        db: Db,
        config: &Config,
        db_path: &Path,
        cache: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type a few words you remember"));
        cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.run_search(cx);
            }
        })
        .detach();

        let mut this = Self {
            db,
            weights: config.weights,
            cache: cache.to_path_buf(),
            input,
            panes: cx.new(|_| ResizableState::default()),
            scroll: VirtualListScrollHandle::new(),
            results: Vec::new(),
            selected: 0,
            kinds: vec![Kind::Video, Kind::Animated, Kind::Still],
            columns: 1,
            hovered: None,
            preview: None,
            preview_task: None,
            preview_muted: true,
            preview_paused: false,
            status: "".into(),
        };
        this.focus_query(window, cx);
        this.run_search(cx);
        this.rescan(config, db_path, cx);
        this
    }

    /// Bring the collection up to date in the background, then show it. Without
    /// this the window shows what the last run indexed.
    fn rescan(&mut self, config: &Config, db_path: &Path, cx: &mut Context<Self>) {
        let roots = config.roots.clone();
        let cache = self.cache.clone();
        let db_path = db_path.to_path_buf();
        let Ok(ignore) = config.ignore_set() else {
            self.status = "the ignore globs are not valid".into();
            return;
        };
        self.status = "scanning".into();
        cx.spawn(async move |this, cx| {
            let scanned = cx
                .background_spawn(async move {
                    let mut db = Db::open(&db_path)?;
                    let stats = index::scan::scan(&mut db, &roots, &ignore, &cache)?;
                    drop(db);
                    index::run(&db_path, &cache)?;
                    anyhow::Ok(stats)
                })
                .await;
            this.update(cx, |this, cx| {
                if let Err(e) = &scanned {
                    this.status = format!("scan failed: {e:#}").into();
                }
                this.run_search(cx);
            })
            .ok();
        })
        .detach();
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let request = search::Request::new(text).kinds(self.kinds.clone());
        match search::search(self.db.conn(), &request, &self.weights) {
            Ok(hits) => {
                self.status = match hits.len() {
                    0 => "nothing found".into(),
                    1 => "1 result".into(),
                    n => format!("{n} results").into(),
                };
                self.results = hits;
            }
            Err(e) => {
                log::error!("search failed: {e:#}");
                self.status = format!("search failed: {e:#}").into();
                self.results.clear();
            }
        }
        self.selected = 0;
        self.hovered = None;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.schedule_preview(cx);
        cx.notify();
    }

    fn selected_hit(&self) -> Option<&Hit> {
        self.results.get(self.selected)
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.results.len() || index == self.selected {
            return;
        }
        self.selected = index;
        self.scroll
            .scroll_to_item(index / self.columns.max(1), ScrollStrategy::Nearest);
        self.schedule_preview(cx);
        cx.notify();
    }

    /// `across` moves along a row, `down` moves a whole row.
    fn move_selection(&mut self, across: isize, down: isize, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let columns = self.columns.max(1) as isize;
        let target = self.selected as isize + across + down * columns;
        let target = target.clamp(0, self.results.len() as isize - 1) as usize;
        self.select(target, cx);
    }

    fn clear_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |state, cx| state.set_value("", window, cx));
        // Setting the value is not typing, so it raises no change event.
        self.run_search(cx);
        self.focus_query(window, cx);
    }

    /// The search box holds focus at all times, so any typed character goes to
    /// it. Focus given in the constructor is lost while the window finishes
    /// opening, so every frame takes it back.
    fn focus_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.input.read(cx).focus_handle(cx);
        if !handle.is_focused(window) {
            window.focus(&handle, cx);
        }
    }

    /// The side pane plays the selection, after it stops moving.
    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        self.preview = None;
        self.preview_task = None;
        let Some(hit) = self.selected_hit() else {
            return;
        };
        if hit.file.kind == Kind::Still {
            return;
        }
        let index = self.selected;
        let path = hit.file.path.clone();
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DELAY).await;
            this.update(cx, |this, cx| {
                if this.selected != index {
                    return;
                }
                this.preview = open_video(&path).inspect(|video| {
                    video.set_muted(this.preview_muted);
                    video.set_paused(this.preview_paused);
                });
                cx.notify();
            })
            .ok();
        }));
    }

    fn toggle_preview_muted(&mut self, cx: &mut Context<Self>) {
        self.preview_muted = !self.preview_muted;
        if let Some(video) = &self.preview {
            video.set_muted(self.preview_muted);
        }
        cx.notify();
    }

    fn toggle_preview_paused(&mut self, cx: &mut Context<Self>) {
        self.preview_paused = !self.preview_paused;
        if let Some(video) = &self.preview {
            video.set_paused(self.preview_paused);
        }
        cx.notify();
    }

    fn set_hovered(&mut self, index: usize, hovered: bool, cx: &mut Context<Self>) {
        let playing = self.hovered.as_ref().map(|(i, _)| *i);
        if !hovered {
            if playing == Some(index) {
                self.hovered = None;
                cx.notify();
            }
            return;
        }
        if playing == Some(index) {
            return;
        }
        let Some(hit) = self.results.get(index) else {
            return;
        };
        // A still image does not react to hover. A tile is always silent.
        self.hovered = match hit.file.kind {
            Kind::Still => None,
            _ => open_video(&hit.file.path).map(|video| {
                video.set_muted(true);
                video.set_paused(false);
                (index, video)
            }),
        };
        cx.notify();
    }

    fn toggle_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if let Some(at) = self.kinds.iter().position(|k| *k == kind) {
            // The last chip stays on. No chips would mean no results at all.
            if self.kinds.len() > 1 {
                self.kinds.remove(at);
            }
        } else {
            self.kinds.push(kind);
        }
        self.run_search(cx);
    }

    /// The width the grid had in the last frame, which decides the columns.
    fn grid_width(&self, window: &Window, cx: &App) -> f32 {
        let sizes = self.panes.read(cx).sizes();
        match sizes.first() {
            Some(width) if *width > px(0.) => f32::from(*width),
            _ => f32::from(window.viewport_size().width) - SIDE_PANE_WIDTH,
        }
    }
}

impl Render for FileBuddy {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.focus_query(window, cx);
        let width = self.grid_width(window, cx);
        self.columns = grid::columns(width);

        v_flex()
            .key_context("FileBuddy")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.move_selection(-1, 0, cx)))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.move_selection(1, 0, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.move_selection(0, -1, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.move_selection(0, 1, cx)))
            .on_action(cx.listener(|this, _: &ClearQuery, window, cx| this.clear_query(window, cx)))
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .p_3()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().flex_1().child(Input::new(&self.input)))
                    .child(chips::chips(self, cx))
                    .child(
                        div()
                            .min_w(px(90.))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    ),
            )
            .child(
                h_resizable("panes")
                    .with_state(&self.panes)
                    .child(
                        resizable_panel()
                            .size(px(width))
                            .child(grid::grid(self, cx)),
                    )
                    .child(
                        resizable_panel()
                            .size(px(SIDE_PANE_WIDTH))
                            .size_range(px(260.)..px(900.))
                            .child(preview::pane(self, window, cx)),
                    ),
            )
    }
}

/// Open a file for playback in a tile or in the side pane. It loops; the caller
/// decides about sound and about pausing. A file FFmpeg cannot open gives
/// `None`, and the tile keeps its thumbnail.
fn open_video(path: &Path) -> Option<Video> {
    let uri = Url::from_file_path(path).ok()?;
    match Video::new(&uri) {
        Ok(video) => {
            video.set_looping(true);
            Some(video)
        }
        Err(e) => {
            log::warn!("cannot play {}: {e}", path.display());
            None
        }
    }
}
