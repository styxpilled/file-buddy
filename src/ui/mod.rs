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
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseDownEvent, ScrollStrategy,
    SharedString, Task, Window, div, px,
};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme, ResizableState, VirtualListScrollHandle, h_flex, h_resizable, resizable_panel,
    v_flex,
};
use gpui_video::{Video, VideoOptions};
use url::Url;

use crate::clipboard;
use crate::config::{Config, Weights};
use crate::db::Db;
use crate::index;
use crate::media::Kind;
use crate::search::{self, Hit};

/// How long the selection must rest before the side pane opens a decoder. Held
/// arrow keys would otherwise open one file per keystroke.
const PREVIEW_DELAY: Duration = Duration::from_millis(180);
/// How long the pane waits for the first frame of the new clip before it shows
/// it anyway. `FIRST_FRAME_TRIES * FIRST_FRAME_POLL` is the whole budget.
const FIRST_FRAME_POLL: Duration = Duration::from_millis(16);
const FIRST_FRAME_TRIES: usize = 75;
/// How long the next file in the direction of travel waits before it opens. It
/// must outlast `PREVIEW_DELAY`, so a held arrow key opens nothing at all.
const PRELOAD_DELAY: Duration = Duration::from_millis(400);
/// Frames the file opened ahead may hold. It is paused, so it stops at the
/// prebuffer anyway, but a cap keeps a large clip from taking the memory of a
/// full buffer for a file the user may never reach.
const PRELOAD_BUFFER: usize = 8;
const PREVIEW_BUFFER: usize = 120;
/// One notch of the wheel, or one press of `ctrl-up`.
const VOLUME_STEP: f64 = 0.05;
/// How long a confirmation stays in the header before the result count comes
/// back. Long enough to read, short enough that it never looks like the state.
const NOTICE_LIFE: Duration = Duration::from_millis(2200);
pub const SIDE_PANE_WIDTH: f32 = 380.;

gpui_kit::actions!(
    file_buddy,
    [
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        FocusQuery,
        ToggleMuted,
        TogglePaused,
        ToggleAutoplay,
        VolumeUp,
        VolumeDown,
        CopyFile,
        RevealFile,
        OpenFile
    ]
);

/// The window keys are plain letters, so they must not reach the search box
/// while it types. `!Input` fails the match when any node on the focus path is
/// an input. Autoplay is the one key that works either way.
pub fn bind_keys(cx: &mut App) {
    let grid = Some("FileBuddy && !Input");
    cx.bind_keys([
        KeyBinding::new("left", SelectLeft, grid),
        KeyBinding::new("right", SelectRight, grid),
        KeyBinding::new("up", SelectUp, grid),
        KeyBinding::new("down", SelectDown, grid),
        KeyBinding::new("f", FocusQuery, grid),
        KeyBinding::new("m", ToggleMuted, grid),
        KeyBinding::new("space", TogglePaused, grid),
        KeyBinding::new("enter", CopyFile, grid),
        KeyBinding::new("ctrl-c", CopyFile, Some("FileBuddy")),
        KeyBinding::new("ctrl-e", RevealFile, Some("FileBuddy")),
        KeyBinding::new("ctrl-o", OpenFile, Some("FileBuddy")),
        KeyBinding::new("ctrl-space", ToggleAutoplay, Some("FileBuddy")),
        KeyBinding::new("ctrl-up", VolumeUp, Some("FileBuddy")),
        KeyBinding::new("ctrl-down", VolumeDown, Some("FileBuddy")),
    ]);
}

pub struct FileBuddy {
    db: Db,
    weights: Weights,
    cache: PathBuf,
    input: Entity<InputState>,
    /// Held while the search box is not focused, so the window keys arrive.
    focus: FocusHandle,
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
    /// The next file in the direction of travel, opened silent and still, with
    /// the index it was opened for. A change of direction discards it.
    preloaded: Option<(usize, Video)>,
    preload_task: Option<Task<()>>,
    /// The last step through the grid, which says which file to open ahead.
    step: isize,
    /// Both kept across selections, so the sound does not come back by itself
    /// and does not return to full.
    preview_muted: bool,
    volume: f64,
    /// Whether a new preview starts playing. Pausing one clip does not change
    /// it, and it never touches a tile.
    autoplay: bool,
    preview_paused: bool,
    status: SharedString,
    /// A confirmation, shown in place of the result count until it expires. The
    /// count is the state; this is the answer to the key that was just pressed.
    notice: Option<Notice>,
    notice_task: Option<Task<()>>,
}

/// What a file action did. The colour is the whole point: a copy that failed
/// and a copy that worked must not read the same at a glance.
pub(super) struct Notice {
    text: SharedString,
    failed: bool,
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
            focus: cx.focus_handle(),
            panes: cx.new(|_| ResizableState::default()),
            scroll: VirtualListScrollHandle::new(),
            results: Vec::new(),
            selected: 0,
            kinds: vec![Kind::Video, Kind::Animated, Kind::Still],
            columns: 1,
            hovered: None,
            preview: None,
            preview_task: None,
            preloaded: None,
            preload_task: None,
            step: 1,
            preview_muted: true,
            volume: 1.,
            autoplay: false,
            preview_paused: true,
            status: "".into(),
            notice: None,
            notice_task: None,
        };
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
        // The index it was opened for means nothing against new results.
        self.preloaded = None;
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
        // One cell or one row is travel, and says what to open ahead. A click
        // across the grid says nothing, so the direction holds.
        let step = index as isize - self.selected as isize;
        if step.abs() <= self.columns.max(1) as isize {
            self.step = step;
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

    fn query_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.read(cx).focus_handle(cx).is_focused(window)
    }

    fn focus_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    fn blur_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.query_focused(window, cx) {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Focus given in the constructor is lost while the window finishes
    /// opening, so every frame takes it back for the window keys.
    fn hold_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) && !self.query_focused(window, cx) {
            window.focus(&self.focus, cx);
        }
    }

    /// The side pane plays the selection, after it stops moving. The clip on
    /// screen stays there, silent, until the new one has a frame to show.
    /// Clearing it first left the stage empty for a moment, which flashed.
    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_task = None;
        let Some(hit) = self.selected_hit() else {
            self.preview = None;
            self.preloaded = None;
            return;
        };
        let kind = hit.file.kind;
        let path = hit.file.path.clone();
        // A still image has no clip of its own, so nothing may linger over it.
        if kind == Kind::Still {
            self.preview = None;
            self.schedule_preload(cx);
            return;
        }
        self.preview_paused = !self.autoplay;

        // The file opened ahead is already decoded, so it costs no wait at all.
        if let Some(video) = self.take_preloaded(self.selected) {
            video.set_muted(self.preview_muted);
            video.set_volume(self.volume);
            video.set_paused(self.preview_paused);
            self.preview = Some(video);
            self.schedule_preload(cx);
            cx.notify();
            return;
        }

        if let Some(outgoing) = &self.preview {
            outgoing.set_muted(true);
        }
        let index = self.selected;
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DELAY).await;
            let opened = this
                .update(cx, |this, _| {
                    open_video(&path, PREVIEW_BUFFER).inspect(|video| {
                        video.set_muted(this.preview_muted);
                        video.set_volume(this.volume);
                        video.set_paused(this.preview_paused);
                    })
                })
                .ok()
                .flatten();
            let Some(video) = opened else {
                // FFmpeg cannot open it, so the thumbnail is all there is.
                this.update(cx, |this, cx| {
                    if this.selected == index {
                        this.preview = None;
                        this.schedule_preload(cx);
                        cx.notify();
                    }
                })
                .ok();
                return;
            };
            // A file that never decodes must not hold the old clip forever.
            for _ in 0..FIRST_FRAME_TRIES {
                if video.frame_seq() > 0 {
                    break;
                }
                cx.background_executor().timer(FIRST_FRAME_POLL).await;
            }
            this.update(cx, |this, cx| {
                if this.selected != index {
                    return;
                }
                this.preview = Some(video);
                this.schedule_preload(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// The file one step ahead, if the grid holds one.
    fn next_index(&self) -> Option<usize> {
        let ahead = usize::try_from(self.selected as isize + self.step).ok()?;
        (ahead < self.results.len()).then_some(ahead)
    }

    fn take_preloaded(&mut self, index: usize) -> Option<Video> {
        match self.preloaded.take() {
            Some((at, video)) if at == index => Some(video),
            _ => None,
        }
    }

    /// Open the next file in the direction of travel, so the arrow key that
    /// reaches it has nothing to wait for. It stays silent and still until
    /// then, which costs one decoder and a few frames.
    fn schedule_preload(&mut self, cx: &mut Context<Self>) {
        self.preload_task = None;
        let Some(index) = self.next_index() else {
            self.preloaded = None;
            return;
        };
        if matches!(&self.preloaded, Some((at, _)) if *at == index) {
            return;
        }
        self.preloaded = None;
        let Some(hit) = self.results.get(index) else {
            return;
        };
        if hit.file.kind == Kind::Still {
            return;
        }
        let path = hit.file.path.clone();
        self.preload_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PRELOAD_DELAY).await;
            this.update(cx, |this, _| {
                // The selection may have moved on while the timer ran.
                if this.next_index() != Some(index) {
                    return;
                }
                this.preloaded = open_video(&path, PRELOAD_BUFFER).map(|video| {
                    video.set_muted(true);
                    video.set_paused(true);
                    (index, video)
                });
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

    /// One step of the level, from the wheel over the pane or from `ctrl-up`.
    /// Turning it up is a request to hear the clip, so it lifts the mute.
    pub(super) fn nudge_volume(&mut self, up: bool, cx: &mut Context<Self>) {
        let step = if up { VOLUME_STEP } else { -VOLUME_STEP };
        self.volume = (self.volume + step).clamp(0., 1.);
        if up && self.volume > 0. {
            self.preview_muted = false;
        }
        if let Some(video) = &self.preview {
            video.set_volume(self.volume);
            video.set_muted(self.preview_muted);
        }
        cx.notify();
    }

    /// The level as the header shows it. A muted clip says so, because the
    /// number alone would not explain the silence.
    fn volume_label(&self) -> SharedString {
        if self.preview_muted {
            return "muted".into();
        }
        format!("{}%", (self.volume * 100.).round() as u32).into()
    }

    /// One clip only. The next selection follows the autoplay flag again.
    fn toggle_preview_paused(&mut self, cx: &mut Context<Self>) {
        self.preview_paused = !self.preview_paused;
        if let Some(video) = &self.preview {
            video.set_paused(self.preview_paused);
        }
        cx.notify();
    }

    fn toggle_autoplay(&mut self, cx: &mut Context<Self>) {
        self.autoplay = !self.autoplay;
        self.preview_paused = !self.autoplay;
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
            _ => open_video(&hit.file.path, PREVIEW_BUFFER).map(|video| {
                video.set_muted(true);
                video.set_paused(false);
                (index, video)
            }),
        };
        cx.notify();
    }

    /// Say what just happened, for a couple of seconds. A second action
    /// replaces the first, so the header never shows a stale answer.
    fn flash(&mut self, text: impl Into<SharedString>, failed: bool, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            text: text.into(),
            failed,
        });
        self.notice_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_LIFE).await;
            this.update(cx, |this, cx| {
                this.notice = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The selected file, if it is still on disk. The index is a snapshot, so a
    /// file can be gone while its tile is still on screen, and every action
    /// against a missing file fails silently in the platform call.
    fn selected_file(&mut self, cx: &mut Context<Self>) -> Option<PathBuf> {
        let path = self.selected_hit()?.file.path.clone();
        if !path.exists() {
            self.flash("the file is gone", true, cx);
            return None;
        }
        Some(path)
    }

    /// Enter. The file goes on the clipboard as a file and as its path, so the
    /// next paste works in an upload box and in a text field alike.
    pub(super) fn copy_selected(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected_file(cx) else {
            return;
        };
        match clipboard::copy_file(&path) {
            Ok(()) => {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.flash(format!("copied {name}"), false, cx);
            }
            Err(e) => {
                log::error!("copy failed: {e:#}");
                self.flash(format!("copy failed: {e}"), true, cx);
            }
        }
    }

    pub(super) fn reveal_selected(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected_file(cx) else {
            return;
        };
        cx.reveal_path(&dunce::canonicalize(path).expect("cannot canonicalize path"));
    }

    pub(super) fn open_selected(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected_file(cx) else {
            return;
        };
        cx.open_with_system(&dunce::canonicalize(path).expect("cannot canonicalize path"));
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
        self.hold_focus(window, cx);
        let width = self.grid_width(window, cx);
        self.columns = grid::columns(width);

        v_flex()
            .key_context("FileBuddy")
            .track_focus(&self.focus)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.move_selection(-1, 0, cx)))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.move_selection(1, 0, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.move_selection(0, -1, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.move_selection(0, 1, cx)))
            .on_action(cx.listener(|this, _: &FocusQuery, window, cx| this.focus_query(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleMuted, _, cx| this.toggle_preview_muted(cx)))
            .on_action(cx.listener(|this, _: &TogglePaused, _, cx| this.toggle_preview_paused(cx)))
            .on_action(cx.listener(|this, _: &ToggleAutoplay, _, cx| this.toggle_autoplay(cx)))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| this.nudge_volume(true, cx)))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| this.nudge_volume(false, cx)))
            .on_action(cx.listener(|this, _: &CopyFile, _, cx| this.copy_selected(cx)))
            .on_action(cx.listener(|this, _: &RevealFile, _, cx| this.reveal_selected(cx)))
            .on_action(cx.listener(|this, _: &OpenFile, _, cx| this.open_selected(cx)))
            // The search box propagates the escape it does not need itself.
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.blur_query(window, cx)))
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .p_3()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .on_mouse_down_out(cx.listener(
                                |this, _: &MouseDownEvent, window, cx| this.blur_query(window, cx),
                            ))
                            .child(Input::new(&self.input)),
                    )
                    .child(chips::chips(self, cx))
                    .child(
                        div()
                            .min_w(px(56.))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.volume_label()),
                    )
                    .child(match &self.notice {
                        Some(notice) => div()
                            .min_w(px(90.))
                            .max_w(px(260.))
                            .truncate()
                            .text_sm()
                            .text_color(if notice.failed {
                                cx.theme().danger
                            } else {
                                cx.theme().success
                            })
                            .child(notice.text.clone()),
                        None => div()
                            .min_w(px(90.))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.status.clone()),
                    }),
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
/// `None`, and the tile keeps its thumbnail. `buffer` caps the decoded frames it
/// holds, which matters because a frame is the full size of the clip.
fn open_video(path: &Path, buffer: usize) -> Option<Video> {
    let uri = Url::from_file_path(path).ok()?;
    let options = VideoOptions {
        frame_buffer_capacity: buffer,
        looping: true,
        ..VideoOptions::default()
    };
    match Video::new_with_options(&uri, options) {
        Ok(video) => Some(video),
        Err(e) => {
            log::warn!("cannot play {}: {e}", path.display());
            None
        }
    }
}
