//! The window. One view owns the state: the query, the results, the selection
//! and the two videos that may play. The files beside this one only draw it.

mod chips;
mod grid;
mod pages;
mod preview;
mod renamer;
mod review;
mod settings;
mod tagger;
mod tile;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseDownEvent, PathPromptOptions,
    ScrollStrategy, SharedString, Task, Window, div, px,
};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme, ResizableState, VirtualListScrollHandle, h_flex, h_resizable, resizable_panel,
    v_flex,
};
use gpui_video::{Video, VideoOptions};
use url::Url;

use self::renamer::Renamer;
use self::settings::Settings;
use self::tagger::{MAX_SUGGESTIONS, Tagger};
use crate::actions::{clipboard, rename};
use crate::config::Config;
use crate::db::review::{self as groups, Group};
use crate::db::{Db, files, tags};
use crate::index;
use crate::index::jobs::{self, Stage};
use crate::media::Kind;
use crate::paths;
use crate::search::{self, Hit, Signal};

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
const HEADER_HEIGHT: f32 = 58.;
const PROGRESS_POLL: Duration = Duration::from_secs(2);
/// How often text that a stage found meanwhile reaches the results.
const INDEX_REFRESH: Duration = Duration::from_secs(30);
/// A decoder closes its file on its own thread, a moment after the clip is
/// dropped, and the rename fails until it has. `RENAME_TRIES * RENAME_RETRY`
/// is how long the rename waits for it.
const RENAME_RETRY: Duration = Duration::from_millis(50);
const RENAME_TRIES: usize = 20;

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
        OpenFile,
        TagFiles,
        ToggleMark,
        Dismiss,
        NextSuggestion,
        PreviousSuggestion,
        RenameFile,
        ShowBrowse,
        ShowReview,
        ShowSettings,
        SaveSettings,
        ReviewDuplicates,
        ReviewLong,
        ReviewFailed,
        ReviewPoorNames
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
        KeyBinding::new("t", TagFiles, grid),
        KeyBinding::new("x", ToggleMark, grid),
        KeyBinding::new("r", RenameFile, grid),
        KeyBinding::new("ctrl-r", RenameFile, Some("FileBuddy")),
        KeyBinding::new("f2", RenameFile, Some("FileBuddy")),
        KeyBinding::new("ctrl-1", ShowBrowse, Some("FileBuddy")),
        KeyBinding::new("ctrl-2", ShowReview, Some("FileBuddy")),
        KeyBinding::new("ctrl-3", ShowSettings, Some("FileBuddy")),
        KeyBinding::new("ctrl-s", SaveSettings, Some("FileBuddy")),
        KeyBinding::new("alt-1", ReviewDuplicates, Some("FileBuddy")),
        KeyBinding::new("alt-2", ReviewLong, Some("FileBuddy")),
        KeyBinding::new("alt-3", ReviewFailed, Some("FileBuddy")),
        KeyBinding::new("alt-4", ReviewPoorNames, Some("FileBuddy")),
        // `input::Escape` reaches this view only while an input has focus, so
        // escape needs a binding of its own for the rest of the time.
        KeyBinding::new("escape", Dismiss, grid),
        // Plain arrows belong to the text in the tag input, so the suggestion
        // list takes the ones the input leaves alone.
        KeyBinding::new("alt-down", NextSuggestion, Some("FileBuddy")),
        KeyBinding::new("alt-up", PreviousSuggestion, Some("FileBuddy")),
        KeyBinding::new("ctrl-space", ToggleAutoplay, Some("FileBuddy")),
        KeyBinding::new("ctrl-up", VolumeUp, Some("FileBuddy")),
        KeyBinding::new("ctrl-down", VolumeDown, Some("FileBuddy")),
    ]);
}

pub struct FileBuddy {
    db: Db,
    db_path: PathBuf,
    /// As it was last read or saved.
    config: Config,
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
    preview: Option<Preview>,
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
    /// Indexing progress, or why a stage cannot run.
    indexing: Option<Notice>,
    /// Files marked for one bulk action, by hash. A hash survives a new search
    /// and a rename; a grid index survives neither.
    marked: HashSet<String>,
    tagger: Option<Tagger>,
    renamer: Option<Renamer>,
    rename_task: Option<Task<()>>,
    /// The Review page, when it is open. Its group replaces the results.
    review: Option<Review>,
    /// The file menu in the side pane is open and holds focus.
    menu_open: bool,
    /// The Settings page, when it is open. It holds the unsaved form.
    settings: Option<Settings>,
    scanning: bool,
    /// Settings saved new folders while a scan ran, so another scan follows it.
    rescan_after: bool,
    /// The expensive stage that runs in the background, if one does.
    indexing_stage: Option<Stage>,
    /// The tab under the preview stage. It stays as the selection moves, so
    /// stepping through the results keeps reading the same kind of thing.
    detail: Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Browse,
    Review,
    Settings,
}

/// What the side pane shows under the clip: the facts of the file, the speech
/// the transcriber heard, or the text the recognizer read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Detail {
    File,
    Speech,
    Text,
}

pub(super) struct Review {
    group: Group,
    /// Files in each group, in the order of `Group::ALL`.
    counts: [usize; 4],
}

/// The clip in the side pane, with the identity of what it holds. Identity is
/// the content hash, the same as everywhere else in the application, so two
/// paths with the same bytes are one video and stepping between them does not
/// start the clip again.
pub(super) struct Preview {
    hash: String,
    video: Video,
}

/// What the header calls a stage while it runs.
fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::Ocr => "text",
        _ => "speech",
    }
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
            db_path: db_path.to_path_buf(),
            config: config.clone(),
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
            preview_muted: false,
            volume: 0.5,
            autoplay: false,
            preview_paused: true,
            status: "".into(),
            notice: None,
            notice_task: None,
            indexing: None,
            marked: HashSet::new(),
            tagger: None,
            renamer: None,
            rename_task: None,
            review: None,
            menu_open: false,
            settings: None,
            scanning: false,
            rescan_after: false,
            indexing_stage: None,
            detail: Detail::File,
        };
        this.run_search(cx);
        this.rescan(cx);
        this
    }

    /// Bring the collection up to date in the background, then show it. Without
    /// this the window shows what the last run indexed.
    /// A second scan at the same time would walk the same files twice, so a
    /// request during a scan waits for it to end.
    fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            self.rescan_after = true;
            return;
        }
        let roots = self.config.roots.clone();
        let cache = self.cache.clone();
        let db_path = self.db_path.clone();
        let Ok(ignore) = self.config.ignore_set() else {
            self.status = "the ignore globs are not valid".into();
            return;
        };
        self.scanning = true;
        self.status = "scanning".into();
        cx.spawn(async move |this, cx| {
            let scanned = cx
                .background_spawn({
                    let db_path = db_path.clone();
                    async move {
                        let mut db = Db::open(&db_path)?;
                        let stats = index::scan::scan(&mut db, &roots, &ignore, &cache)?;
                        drop(db);
                        index::run(&db_path, &cache)?;
                        anyhow::Ok(stats)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.scanning = false;
                this.run_search(cx);
                match &scanned {
                    Err(e) => this.status = format!("scan failed: {e:#}").into(),
                    Ok(_) => this.index_text(Stage::Ocr, db_path, cx),
                }
                if std::mem::take(&mut this.rescan_after) {
                    this.rescan(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Run one expensive stage in the background and show how far it is. OCR
    /// comes first and speech follows it, because OCR is minutes of work where
    /// speech is hours. The queue is durable, so a restart continues it, and a
    /// run that is already going picks up the jobs a later scan queues.
    fn index_text(&mut self, stage: Stage, db_path: PathBuf, cx: &mut Context<Self>) {
        if self.indexing_stage.is_some() {
            return;
        }
        if stage == Stage::Whisper
            && let Err(e) = paths::whisper_model()
        {
            log::error!("{e:#}");
            self.indexing = Some(Notice {
                text: format!(
                    "speech model missing: {}\\{}",
                    paths::MODEL_DIR,
                    paths::WHISPER_MODEL
                )
                .into(),
                failed: true,
            });
            cx.notify();
            return;
        }

        self.indexing_stage = Some(stage);
        let finished = Arc::new(AtomicBool::new(false));
        let work = cx.background_spawn({
            let cache = self.cache.clone();
            let db_path = db_path.clone();
            let finished = finished.clone();
            async move {
                let result = match stage {
                    Stage::Ocr => index::read_text(&db_path, &cache),
                    _ => index::transcribe(&db_path, &cache),
                };
                finished.store(true, Ordering::Relaxed);
                result
            }
        });
        cx.spawn(async move |this, cx| {
            let mut refreshed_at = Instant::now();
            let mut refreshed_count = None;
            while !finished.load(Ordering::Relaxed) {
                let alive = this.update(cx, |this, cx| {
                    let Ok(progress) = jobs::progress(this.db.conn(), stage) else {
                        return;
                    };
                    if progress.finished < progress.total {
                        this.indexing = Some(Notice {
                            text: format!(
                                "{} {} / {}",
                                stage_label(stage),
                                progress.finished,
                                progress.total
                            )
                            .into(),
                            failed: false,
                        });
                    }
                    // New text reaches the results now and then, not on every
                    // file, so the grid does not keep moving.
                    if refreshed_count != Some(progress.finished)
                        && refreshed_at.elapsed() >= INDEX_REFRESH
                    {
                        refreshed_at = Instant::now();
                        refreshed_count = Some(progress.finished);
                        this.rerun_search(cx);
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
                cx.background_executor().timer(PROGRESS_POLL).await;
            }
            let result = work.await;
            this.update(cx, |this, cx| {
                this.indexing_stage = None;
                this.indexing = match result {
                    Ok(_) => None,
                    Err(e) => {
                        log::error!("{} failed: {e:#}", stage.as_str());
                        Some(Notice {
                            text: format!("{} failed: {e:#}", stage_label(stage)).into(),
                            failed: true,
                        })
                    }
                };
                this.rerun_search(cx);
                if stage == Stage::Ocr {
                    this.index_text(Stage::Whisper, db_path, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn fetch_results(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let request = search::Request::new(text).kinds(self.kinds.clone());
        match search::search(self.db.conn(), &request, &self.config.weights) {
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
    }

    /// A new query. Everything that was about the old results goes with them.
    /// Review holds its own results, which a background refresh must not
    /// replace.
    fn run_search(&mut self, cx: &mut Context<Self>) {
        if self.review.is_some() {
            return;
        }
        self.fetch_results(cx);
        self.selected = 0;
        self.hovered = None;
        // The index it was opened for means nothing against new results.
        self.preloaded = None;
        // A mark is a promise about a file the user can see. New results are a
        // different set of files, so the promise cannot be kept.
        self.marked.clear();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.schedule_preview(cx);
        cx.notify();
    }

    /// The same query again, after the index changed under it. A tag can move
    /// a file up the ranking, and dropping the user back at the top of the
    /// grid mid-tagging would lose their place.
    fn rerun_search(&mut self, cx: &mut Context<Self>) {
        if self.review.is_some() {
            return;
        }
        let was = self.selected_hit().map(|hit| hit.file.hash.clone());
        self.fetch_results(cx);

        // Follow the file, not the row. A tag reorders the results, so the row
        // the selection sat on can now hold something else, and taking the tag
        // that found a file off it drops the file out of the results entirely.
        self.selected = was
            .and_then(|hash| self.results.iter().position(|hit| hit.file.hash == hash))
            .unwrap_or_else(|| self.selected.min(self.results.len().saturating_sub(1)));

        // Both of these are about files that were on screen a moment ago. A
        // mark on a file that is no longer a result cannot be seen, so counting
        // it in the header would be a lie, and the file opened ahead was opened
        // for a row that now holds something else.
        self.marked.retain(|hash| {
            self.results
                .iter()
                .any(|hit| hit.file.hash.as_str() == hash.as_str())
        });
        self.preloaded = None;

        // The selection can land on a different file even when the row number
        // does not change, and the pane would otherwise keep playing the clip
        // that is no longer selected. Clicking the tile could not fix that:
        // `select` is right to do nothing when the row has not moved.
        self.schedule_preview(cx);
        cx.notify();
    }

    fn selected_hit(&self) -> Option<&Hit> {
        self.results.get(self.selected)
    }

    pub(super) fn preview_video(&self) -> Option<&Video> {
        self.preview.as_ref().map(|preview| &preview.video)
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
        if self.page() != Page::Browse {
            return;
        }
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
        if !self.focus.is_focused(window)
            && !self.query_focused(window, cx)
            && self.tagger.is_none()
            && self.renamer.is_none()
            && self.settings.is_none()
            && !self.menu_open
        {
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
        let hash = hit.file.hash.clone();
        // A still image has no clip of its own, so nothing may linger over it.
        if kind == Kind::Still {
            self.preview = None;
            self.schedule_preload(cx);
            return;
        }

        // Already playing this one. A search runs on every keystroke and puts
        // the same file back under the selection, and a step onto a duplicate
        // lands on the same bytes, so reopening here would throw away a warm
        // decoder and start the clip again from nothing for no reason at all.
        // Whether it is playing and how loud it is are the user's answers to a
        // question about this clip, so they stand too.
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.hash == hash)
        {
            self.schedule_preload(cx);
            return;
        }

        // The walk asks the user to name what they see, so the clip plays.
        self.preview_paused = !(self.autoplay || self.walking());

        // The file opened ahead is already decoded, so it costs no wait at all.
        if let Some(video) = self.take_preloaded(self.selected) {
            video.set_muted(self.preview_muted);
            video.set_volume(self.volume);
            video.set_paused(self.preview_paused);
            self.preview = Some(Preview { hash, video });
            self.schedule_preload(cx);
            cx.notify();
            return;
        }

        if let Some(outgoing) = &self.preview {
            outgoing.video.set_muted(true);
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
                this.preview = Some(Preview { hash, video });
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
        if let Some(video) = self.preview_video() {
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
        if let Some(video) = self.preview_video() {
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
        if let Some(video) = self.preview_video() {
            video.set_paused(self.preview_paused);
        }
        cx.notify();
    }

    fn toggle_autoplay(&mut self, cx: &mut Context<Self>) {
        self.autoplay = !self.autoplay;
        self.preview_paused = !self.autoplay;
        if let Some(video) = self.preview_video() {
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

    /// The files a bulk action works on: everything marked, or the selection
    /// when nothing is marked. Acting on the selection while marks exist would
    /// be a quiet way to tag the wrong file.
    fn action_targets(&self) -> Vec<String> {
        if !self.marked.is_empty() {
            let mut hashes: Vec<String> = self.marked.iter().cloned().collect();
            hashes.sort();
            return hashes;
        }
        self.selected_hit()
            .map(|hit| vec![hit.file.hash.clone()])
            .unwrap_or_default()
    }

    pub(super) fn toggle_mark(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(hit) = self.results.get(index) else {
            return;
        };
        let hash = hit.file.hash.clone();
        if !self.marked.remove(&hash) {
            self.marked.insert(hash);
        }
        cx.notify();
    }

    fn clear_marks(&mut self, cx: &mut Context<Self>) {
        if self.marked.is_empty() {
            return;
        }
        self.marked.clear();
        cx.notify();
    }

    pub(super) fn is_marked(&self, hit: &Hit) -> bool {
        self.marked.contains(&hit.file.hash)
    }

    /// Open the tag input on the marked files, or on the selection.
    fn open_tagger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.action_targets();
        if targets.is_empty() {
            return;
        }
        let subject: SharedString = match (self.marked.len(), self.selected_hit()) {
            (0, Some(hit)) => format!("Tag {}", hit.file.filename).into(),
            (1, _) => "Tag 1 marked file".into(),
            (n, _) => format!("Tag {n} marked files").into(),
        };

        let input = cx.new(|cx| InputState::new(window, cx).placeholder("a tag, lowercase"));
        let subscriptions = vec![cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.refresh_suggestions(cx),
                InputEvent::PressEnter { .. } => {
                    let typed = input.read(cx).value().to_string();
                    let chosen = this
                        .tagger
                        .as_ref()
                        .and_then(|tagger| tagger.chosen(&typed));
                    if let Some(name) = chosen {
                        this.apply_tag(name, window, cx);
                    }
                }
                _ => {}
            },
        )];

        let handle = input.read(cx).focus_handle(cx);
        self.tagger = Some(Tagger {
            input,
            subject,
            current: self.common_tags(&targets),
            targets,
            suggestions: Vec::new(),
            highlighted: None,
            _subscriptions: subscriptions,
        });
        self.refresh_suggestions(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Escape, wherever it comes from. It undoes one thing at a time, the most
    /// recent first, so a person leaning on it ends up back at a plain grid.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tagger.is_some() {
            self.close_tagger(window, cx);
        } else if self.renamer.is_some() {
            self.close_renamer(window, cx);
        } else if self.settings.is_some() && !self.focus.is_focused(window) {
            // A field of the form has focus. Leaving would drop what was typed.
            window.focus(&self.focus, cx);
        } else if self.page() != Page::Browse {
            self.show_page(Page::Browse, window, cx);
        } else if self.query_focused(window, cx) {
            self.blur_query(window, cx);
        } else {
            self.clear_marks(cx);
        }
    }

    fn close_tagger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tagger.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn common_tags(&self, targets: &[String]) -> Vec<SharedString> {
        tags::common_to(self.db.conn(), targets)
            .unwrap_or_else(|e| {
                log::error!("reading the tags failed: {e:#}");
                Vec::new()
            })
            .into_iter()
            .map(SharedString::from)
            .collect()
    }

    /// The existing tags that match what is typed. The first is highlighted, so
    /// a tag that is already there is what Enter applies, not a near miss of it.
    fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        let Some(tagger) = &self.tagger else { return };
        let typed = tagger.input.read(cx).value().trim().to_lowercase();
        let found = tags::matching(self.db.conn(), &typed, MAX_SUGGESTIONS).unwrap_or_default();
        let exact = found.contains(&typed);

        let Some(tagger) = &mut self.tagger else {
            return;
        };
        tagger.suggestions = found.into_iter().map(SharedString::from).collect();
        // Nothing typed is a list to browse, not an answer. A name that is
        // already a tag needs no suggestion either: Enter would do the same.
        tagger.highlighted =
            (!typed.is_empty() && !tagger.suggestions.is_empty() && !exact).then_some(0);
        cx.notify();
    }

    fn move_highlight(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(tagger) = &mut self.tagger else {
            return;
        };
        let count = tagger.suggestions.len();
        if count == 0 {
            return;
        }
        // Off either end is what was typed, so a new tag is always reachable.
        tagger.highlighted = match (tagger.highlighted, forward) {
            (None, true) => Some(0),
            (None, false) => Some(count - 1),
            (Some(at), true) if at + 1 < count => Some(at + 1),
            (Some(_), true) => None,
            (Some(0), false) => None,
            (Some(at), false) => Some(at - 1),
        };
        cx.notify();
    }

    /// Apply one tag to every target. The panel stays open with an empty box,
    /// because a file usually wants more than one tag.
    pub(super) fn apply_tag(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let name = name.trim().to_lowercase();
        let Some(tagger) = &self.tagger else { return };
        let targets = tagger.targets.clone();
        if name.is_empty() {
            return;
        }

        if let Err(e) = tags::attach_many(self.db.conn_mut(), &targets, &name) {
            log::error!("tagging failed: {e:#}");
            self.flash(format!("tagging failed: {e}"), true, cx);
            return;
        }
        self.flash(
            format!("tagged {} with {name}", counted(targets.len())),
            false,
            cx,
        );
        self.after_tag_change(window, cx);
    }

    pub(super) fn remove_tag(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tagger) = &self.tagger else { return };
        let targets = tagger.targets.clone();

        if let Err(e) = tags::detach_many(self.db.conn_mut(), &targets, &name) {
            log::error!("untagging failed: {e:#}");
            self.flash(format!("untagging failed: {e}"), true, cx);
            return;
        }
        self.flash(
            format!("took {name} off {}", counted(targets.len())),
            false,
            cx,
        );
        self.after_tag_change(window, cx);
    }

    /// The index changed under the results, so run the query again. The panel
    /// keeps its targets and empties its box for the next tag.
    fn after_tag_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tagger) = &self.tagger else { return };
        let input = tagger.input.clone();
        let targets = tagger.targets.clone();

        input.update(cx, |input, cx| input.set_value("", window, cx));
        let current = self.common_tags(&targets);
        if let Some(tagger) = &mut self.tagger {
            tagger.current = current;
        }
        self.rerun_search(cx);
        self.refresh_suggestions(cx);
    }

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
        // The input takes the place of the name, which only the File tab shows.
        self.detail = Detail::File;

        let input =
            cx.new(|cx| InputState::new(window, cx).default_value(rename::stem(&file.path)));
        input.update(cx, |input, cx| input.select_all(window, cx));
        let subscriptions =
            vec![
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.save_rename(window, cx);
                    }
                }),
            ];

        let handle = input.read(cx).focus_handle(cx);
        self.renamer = Some(Renamer {
            input,
            extension: file
                .path
                .extension()
                .map(|extension| format!(".{}", extension.to_string_lossy()).into())
                .unwrap_or_default(),
            file,
            saving: false,
            _subscriptions: subscriptions,
        });
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Escape, or a click on another tile outside the walk.
    fn close_renamer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renamer = None;
        self.rename_task = None;
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

    /// Every clip that has these bytes open. FFmpeg opens a file without
    /// delete sharing, so Windows refuses the rename while one of them plays.
    fn release_videos(&mut self, hash: &str) {
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.hash == hash)
        {
            self.preview = None;
            self.preview_task = None;
        }
        let results = &self.results;
        let holds = |index: &usize| results.get(*index).is_some_and(|hit| hit.file.hash == hash);
        if self.hovered.as_ref().is_some_and(|(at, _)| holds(at)) {
            self.hovered = None;
        }
        if self.preloaded.as_ref().is_some_and(|(at, _)| holds(at)) {
            self.preloaded = None;
        }
    }

    fn save_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renamer) = &mut self.renamer else {
            return;
        };
        if renamer.saving {
            return;
        }
        renamer.saving = true;
        let typed = renamer.input.read(cx).value().to_string();
        let file = renamer.file.clone();
        self.release_videos(&file.hash);
        cx.notify();

        self.rename_task = Some(cx.spawn_in(window, async move |this, cx| {
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
        }));
    }

    fn finish_rename(
        &mut self,
        file: &files::FileRow,
        result: anyhow::Result<Option<PathBuf>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = match result {
            Ok(target) => target,
            Err(e) => {
                log::error!("rename of {} failed: {e:#}", file.path.display());
                if let Some(renamer) = &mut self.renamer {
                    renamer.saving = false;
                }
                self.flash(e.to_string(), true, cx);
                self.schedule_preview(cx);
                return;
            }
        };
        if let Some(target) = &target {
            let name = files::FileRow::name_of(target);
            self.flash(format!("renamed to {name}"), false, cx);
        }

        if !self.walking() {
            self.renamer = None;
            window.focus(&self.focus, cx);
            if self.review.is_some() {
                self.reload_review(cx);
            } else {
                self.rerun_search(cx);
            }
            return;
        }
        self.count_groups();
        if let (Some(target), Some(hit)) = (
            target,
            self.results.iter_mut().find(|hit| hit.file.id == file.id),
        ) {
            hit.file.filename = files::FileRow::name_of(&target);
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

    /// The Poor names group is the rename walk: the clip plays, and a saved
    /// name moves the renamer on to the next file.
    fn walking(&self) -> bool {
        self.review
            .as_ref()
            .is_some_and(|review| review.group == Group::PoorNames)
    }

    /// `ctrl-shift-r`. Review opens on the first group that holds a file.
    /// Review opens on the first group that holds a file.
    fn open_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.review = Some(Review {
            group: Group::Duplicates,
            counts: [0; 4],
        });
        self.count_groups();
        let counts = self
            .review
            .as_ref()
            .map(|review| review.counts)
            .unwrap_or_default();
        let group = Group::ALL
            .into_iter()
            .zip(counts)
            .find(|(_, count)| *count > 0)
            .map_or(Group::Duplicates, |(group, _)| group);
        self.show_group(group, window, cx);
    }

    pub(super) fn review_group(&self) -> Option<Group> {
        self.review.as_ref().map(|review| review.group)
    }

    fn count_groups(&mut self) {
        let Some(review) = &mut self.review else {
            return;
        };
        for (at, group) in Group::ALL.into_iter().enumerate() {
            match groups::items(self.db.conn(), group) {
                Ok(items) => review.counts[at] = items.len(),
                Err(e) => log::error!("counting {} failed: {e:#}", group.label()),
            }
        }
    }

    fn group_hits(&mut self, group: Group, cx: &mut Context<Self>) -> Vec<Hit> {
        match groups::items(self.db.conn(), group) {
            Ok(items) => items
                .into_iter()
                .map(|item| Hit {
                    file: item.file,
                    matched: item.note,
                    score: 0.0,
                    signal: Signal::Filename,
                })
                .collect(),
            Err(e) => {
                log::error!("reading {} failed: {e:#}", group.label());
                self.flash(format!("reading {} failed: {e}", group.label()), true, cx);
                Vec::new()
            }
        }
    }

    /// A tab of Review, from a click or `ctrl-1` to `ctrl-4`. It opens Review
    /// when it is closed.
    pub(super) fn show_group(&mut self, group: Group, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = None;
        if self.review.is_none() {
            self.review = Some(Review {
                group,
                counts: [0; 4],
            });
        }
        if let Some(review) = &mut self.review {
            review.group = group;
        }
        self.count_groups();
        self.tagger = None;
        self.renamer = None;
        self.rename_task = None;
        self.results = self.group_hits(group, cx);
        self.selected = 0;
        self.step = 1;
        self.hovered = None;
        self.preloaded = None;
        self.marked.clear();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        window.focus(&self.focus, cx);

        if self.walking() && !self.results.is_empty() {
            // The clip on screen may already be the first file, and the
            // preview keeps that one as it is.
            self.preview_paused = false;
            if let Some(preview) = &self.preview
                && preview.hash == self.results[0].file.hash
            {
                preview.video.set_paused(false);
            }
            self.schedule_preview(cx);
            self.open_renamer(window, cx);
        } else {
            self.schedule_preview(cx);
        }
        cx.notify();
    }

    /// The group again, after a rename changed it. The selection follows the
    /// file, as it does in a search.
    fn reload_review(&mut self, cx: &mut Context<Self>) {
        let Some(group) = self.review.as_ref().map(|review| review.group) else {
            return;
        };
        let was = self.selected_hit().map(|hit| hit.file.id);
        self.results = self.group_hits(group, cx);
        self.selected = was
            .and_then(|id| self.results.iter().position(|hit| hit.file.id == id))
            .unwrap_or_else(|| self.selected.min(self.results.len().saturating_sub(1)));
        self.preloaded = None;
        self.hovered = None;
        self.count_groups();
        self.schedule_preview(cx);
        cx.notify();
    }

    pub(super) fn show_detail(&mut self, detail: Detail, cx: &mut Context<Self>) {
        self.detail = detail;
        cx.notify();
    }

    pub(super) fn page(&self) -> Page {
        if self.review.is_some() {
            Page::Review
        } else if self.settings.is_some() {
            Page::Settings
        } else {
            Page::Browse
        }
    }

    /// A page tab, or `ctrl-1` to `ctrl-3`.
    pub(super) fn show_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if page == self.page() {
            return;
        }
        self.tagger = None;
        self.renamer = None;
        self.rename_task = None;
        window.focus(&self.focus, cx);
        match page {
            Page::Browse => {
                self.review = None;
                self.settings = None;
                self.run_search(cx);
            }
            Page::Review => {
                self.settings = None;
                self.open_review(window, cx);
            }
            Page::Settings => {
                self.review = None;
                self.settings = Some(Settings::new(&self.config, window, cx));
                // Nothing plays behind the form, and no key acts on a file that
                // cannot be seen.
                self.results.clear();
                self.selected = 0;
                self.marked.clear();
                self.hovered = None;
                self.preloaded = None;
                self.preview = None;
                self.preview_task = None;
                cx.notify();
            }
        }
    }

    /// Check the form, write the file, and use it. New weights reach the next
    /// search; new folders or globs start a scan.
    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        let config = match settings.edited(cx) {
            Ok(config) => config,
            Err(e) => {
                self.flash(e.to_string(), true, cx);
                return;
            }
        };
        if let Err(e) = paths::config_path().and_then(|path| config.write(&path)) {
            log::error!("saving the settings failed: {e:#}");
            self.flash(format!("saving failed: {e}"), true, cx);
            return;
        }
        let rescan = config.roots != self.config.roots || config.ignore != self.config.ignore;
        self.config = config;
        if rescan {
            self.flash("saved, scanning the folders", false, cx);
            self.rescan(cx);
        } else {
            self.flash("saved", false, cx);
        }
    }

    fn revert_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            self.settings = Some(Settings::new(&self.config, window, cx));
            cx.notify();
        }
    }

    fn add_roots(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            this.update(cx, |this, cx| {
                let Some(settings) = &mut this.settings else {
                    return;
                };
                for folder in folders {
                    if !settings.roots.contains(&folder) {
                        settings.roots.push(folder);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn remove_root(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(settings) = &mut self.settings
            && at < settings.roots.len()
        {
            settings.roots.remove(at);
            cx.notify();
        }
    }

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
            .relative()
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
            .on_action(cx.listener(|this, _: &TagFiles, window, cx| this.open_tagger(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleMark, _, cx| {
                let selected = this.selected;
                this.toggle_mark(selected, cx)
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.dismiss(window, cx)))
            .on_action(
                cx.listener(|this, _: &RenameFile, window, cx| this.open_renamer(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowBrowse, window, cx| {
                this.show_page(Page::Browse, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowReview, window, cx| {
                this.show_page(Page::Review, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
                this.show_page(Page::Settings, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SaveSettings, _, cx| this.save_settings(cx)))
            .on_action(cx.listener(|this, _: &ReviewDuplicates, window, cx| {
                this.show_group(Group::Duplicates, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReviewLong, window, cx| {
                this.show_group(Group::Long, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReviewFailed, window, cx| {
                this.show_group(Group::Failed, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReviewPoorNames, window, cx| {
                this.show_group(Group::PoorNames, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextSuggestion, _, cx| this.move_highlight(true, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousSuggestion, _, cx| this.move_highlight(false, cx)),
            )
            // The search box propagates the escape it does not need itself.
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.dismiss(window, cx)))
            .child(
                h_flex()
                    .w_full()
                    // One height for every page, so the body does not jump when
                    // the search box comes and goes.
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .gap_3()
                    .px_3()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(pages::tabs(self, cx))
                    .map(|header| match (&self.review, &self.settings) {
                        (Some(review), _) => header.child(review::tabs(review, cx)),
                        (None, Some(_)) => header.child(div().flex_1()),
                        (None, None) => header
                            .child(
                                div()
                                    .flex_1()
                                    .on_mouse_down_out(cx.listener(
                                        |this, _: &MouseDownEvent, window, cx| {
                                            this.blur_query(window, cx)
                                        },
                                    ))
                                    .child(Input::new(&self.input)),
                            )
                            .child(chips::chips(self, cx)),
                    })
                    .when(!self.marked.is_empty(), |header| {
                        header.child(
                            div()
                                .px_1()
                                .rounded(px(3.))
                                .bg(cx.theme().success.opacity(0.18))
                                .text_color(cx.theme().success)
                                .text_xs()
                                .child(format!("{} marked", self.marked.len())),
                        )
                    })
                    .when(self.settings.is_none(), |header| {
                        header.child(
                            div()
                                .min_w(px(56.))
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(self.volume_label()),
                        )
                    })
                    .when_some(self.indexing.as_ref(), |header, indexing| {
                        header.child(
                            div()
                                .max_w(px(320.))
                                .truncate()
                                .text_sm()
                                .text_color(if indexing.failed {
                                    cx.theme().danger
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .child(indexing.text.clone()),
                        )
                    })
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
                            .child(match &self.review {
                                Some(_) if self.walking() && !self.results.is_empty() => {
                                    format!("naming {} / {}", self.selected + 1, self.results.len())
                                        .into()
                                }
                                Some(_) => counted(self.results.len()).into(),
                                None if self.settings.is_some() => SharedString::default(),
                                None => self.status.clone(),
                            }),
                    }),
            )
            .when_some(self.settings.as_ref(), |page, _| {
                page.child(settings::page(self, cx))
            })
            .when(self.settings.is_none(), |page| {
                page.child(
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
            })
            .child(tagger::panel(self, cx))
    }
}

/// gpui-kit's `Button` sets the arrow cursor on itself as it renders, so a
/// cursor set on the button is lost. The window takes the cursor of the last
/// element painted under the pointer, so a layer painted over the button wins,
/// and it takes no clicks from it. A disabled button keeps the arrow.
pub(super) fn with_pointer(button: impl IntoElement, enabled: bool) -> gpui::Div {
    div()
        .relative()
        .flex_none()
        .child(button)
        .when(enabled, |wrapper| {
            wrapper.child(div().absolute().inset_0().cursor_pointer())
        })
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

/// `1 file` or `3 files`, for a confirmation that says what it touched.
fn counted(files: usize) -> String {
    match files {
        1 => "1 file".to_string(),
        n => format!("{n} files"),
    }
}
