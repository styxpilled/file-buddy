//! The window. One view, `FileBuddy`, owns the state. Each file beside this one
//! holds one part of the window: the methods that change its state and the
//! functions that draw it.

mod autoscroll;
mod delete;
mod folders;
mod grid;
mod header;
mod history;
mod indexing;
mod keys;
mod player;
mod preview;
mod renamer;
mod review;
mod rows;
mod seek_bar;
mod settings;
#[cfg(feature = "sim")]
pub mod sim;
mod studio;
mod table;
mod tagger;
mod thumbnails;
mod tile;
mod visual;
mod when;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, Context, ElementId, Entity, FocusHandle, Focusable, KeyUpEvent, ScrollStrategy,
    SharedString, Subscription, Task, Window, div, px,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme, ResizableState, Selectable, VirtualListScrollHandle, h_resizable, resizable_panel,
    v_flex,
};

use self::history::Travel;
use self::indexing::Indexer;
use self::player::{Player, PressFrom};
use self::renamer::Renamer;
use self::review::Review;
use self::rows::RowEditor;
use self::seek_bar::{Clip, SeekBar};
use self::settings::Settings;
use self::studio::Studio;
use self::tagger::Tagger;
use self::thumbnails::Thumbnails;
use self::visual::Visual;
use crate::actions::clipboard;
use crate::actions::delete::Mode as DeleteMode;
use crate::config::Config;
use crate::db::review::Group;
use crate::db::{Db, tags};
use crate::media::Kind;
use crate::search::folder::{Entry, Sort};
use crate::search::{self, Hit};

const SIDE_PANE_WIDTH: f32 = 390.;
/// How long a confirmation stays in the header before the result count comes
/// back. Long enough to read, short enough that it never looks like the state.
const NOTICE_LIFE: Duration = Duration::from_millis(2200);

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
        Activate,
        GoUp,
        CycleSort,
        ToggleTable,
        RevealFile,
        OpenFile,
        TagFiles,
        ToggleMark,
        Dismiss,
        NextSuggestion,
        PreviousSuggestion,
        RenameFile,
        ShowBrowse,
        ShowFolders,
        ShowReview,
        ShowSettings,
        SaveSettings,
        ReviewDuplicates,
        ReviewSameNames,
        ReviewLong,
        ReviewFailed,
        ReviewPoorNames,
        Rescan,
        MoreLikeThis,
        SkipBack,
        SkipForward,
        CycleDetail,
        EditRows,
        RowUp,
        RowDown,
        EditRow,
        DeleteRow,
        AddRow,
        KeyRowUp,
        KeyRowDown,
        CaptureKey,
        RemoveKey,
        ResetKeys,
        HistoryBack,
        HistoryForward,
        TrashFiles,
        DeleteFiles,
        ShowStudio,
        OpenInStudio,
        SetIn,
        SetOut,
        ExportEdit
    ]
);

/// Bind every key: the fixed ones, the defaults and the user's changes.
/// Returns the changes that could not apply.
pub fn bind_keys(changes: &BTreeMap<String, Vec<String>>, cx: &mut App) -> Vec<String> {
    keys::apply(changes, cx)
}

pub struct FileBuddy {
    db: Db,
    db_path: PathBuf,
    /// As it was last read or saved.
    config: Config,
    cache: PathBuf,
    /// The page, with the state only that page has.
    mode: Mode,
    input: Entity<InputState>,
    /// What the search box shows while it is empty. It changes with the folder.
    placeholder: SharedString,
    /// Held while the search box is not focused, so the window keys arrive.
    focus: FocusHandle,
    panes: Entity<ResizableState>,
    scroll: VirtualListScrollHandle,
    autoscroll: Option<autoscroll::Autoscroll>,
    results: Vec<Entry>,
    /// Nothing is typed, so the results are a list and not the answer to a
    /// query: the newest files on Browse, or a folder on the Folders page.
    listing: bool,
    selected: usize,
    /// The chips that are on. All three by default.
    kinds: Vec<Kind>,
    /// Tiles across the grid, from the width of the last frame.
    columns: usize,
    player: Player,
    seek_bar: SeekBar,
    /// The edit, which stays when the page closes.
    studio: Studio,
    indexer: Indexer,
    status: SharedString,
    /// A confirmation, shown in place of the result count until it expires. The
    /// count is the state; this is the answer to the key that was just pressed.
    notice: Option<Notice>,
    notice_task: Option<Task<()>>,
    /// Files marked for one bulk action, by row id. An id survives a new search
    /// and a rename; a grid index survives neither. A hash would mark every
    /// copy of a duplicate at once.
    marked: HashSet<i64>,
    tagger: Option<Tagger>,
    renamer: Option<Renamer>,
    /// The cursor on the rows of the Speech or Text tab.
    rows: Option<RowEditor>,
    /// The entries of `[keys]` that could not apply, for the header.
    key_problems: Vec<String>,
    /// Takes the next key while the Keys section of Settings captures one.
    _key_capture: Subscription,
    /// The file menu in the side pane is open and holds focus.
    menu_open: bool,
    /// The folder the Folders page shows, or `None` for the list of roots. It
    /// stays when the page closes, so the page opens where the user left it.
    folder: Option<PathBuf>,
    /// The order of a folder on the Folders page, and of Browse with nothing
    /// typed. A query keeps its ranking.
    folder_sort: Sort,
    browse_sort: Sort,
    /// A table in place of the grid, on every page with results.
    table: bool,
    /// The tags of each file, by hash, while the table shows them.
    row_tags: HashMap<String, Vec<String>>,
    /// The tab under the preview stage. It stays as the selection moves, so
    /// stepping through the results keeps reading the same kind of thing.
    detail: Detail,
    visual: Visual,
    travel: Travel,
    thumbnails: Entity<Thumbnails>,
    /// The bytes of the files a delete has in the shell. No clip opens them
    /// until it is done, and a second delete waits.
    deleting: Vec<String>,
}

/// The status line for a number of results.
fn count(results: usize) -> SharedString {
    match results {
        0 => "nothing found".into(),
        1 => "1 result".into(),
        n => format!("{n} results").into(),
    }
}

enum Mode {
    Browse,
    Folders,
    /// Its group replaces the results.
    Review(Review),
    /// The unsaved form.
    Settings(Box<Settings>),
    Studio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Browse,
    Folders,
    Review,
    Settings,
    Studio,
}

/// What the side pane shows under the name: the speech the transcriber heard,
/// or the text the recognizer read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Detail {
    Speech,
    Text,
}

impl Detail {
    /// `i`.
    fn next(self) -> Self {
        match self {
            Detail::Speech => Detail::Text,
            Detail::Text => Detail::Speech,
        }
    }
}

/// A line in the header. The colour is the whole point: a copy that failed and
/// a copy that worked must not read the same at a glance.
struct Notice {
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
            cx.new(|cx| InputState::new(window, cx).placeholder(folders::BROWSE_PLACEHOLDER));
        // `set_value` sends no `Change`, so this is the user typing. Enter hands
        // the keys back to the results.
        cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.end_likes();
                    this.run_search(cx);
                }
                InputEvent::PressEnter { .. } => this.blur_query(window, cx),
                _ => {}
            },
        )
        .detach();

        let thumbnails = Thumbnails::new(cx);
        cx.observe(&thumbnails, |_, _, cx| cx.notify()).detach();

        // A press held while the window loses focus gets no release.
        cx.observe_window_activation(window, |this: &mut Self, window, cx| {
            if this.player.pressed() && !window.is_window_active() {
                this.player.cancel_press();
                cx.notify();
            }
        })
        .detach();

        let mut this = Self {
            db,
            db_path: db_path.to_path_buf(),
            config: config.clone(),
            cache: cache.to_path_buf(),
            mode: Mode::Browse,
            input,
            placeholder: folders::BROWSE_PLACEHOLDER.into(),
            focus: cx.focus_handle(),
            panes: cx.new(|_| ResizableState::default()),
            scroll: VirtualListScrollHandle::new(),
            autoscroll: None,
            results: Vec::new(),
            listing: false,
            selected: 0,
            kinds: vec![Kind::Video, Kind::Animated, Kind::Still],
            columns: 1,
            player: Player::default(),
            seek_bar: SeekBar::new(Clip::Pane, cx),
            studio: Studio::new(cx),
            indexer: Indexer::new(config.paused),
            status: "".into(),
            notice: None,
            notice_task: None,
            marked: HashSet::new(),
            tagger: None,
            renamer: None,
            rows: None,
            key_problems: keys::resolve(&config.keys).problems(),
            _key_capture: settings::capture_keys(cx),
            menu_open: false,
            folder: None,
            folder_sort: Sort::Name,
            browse_sort: Sort::Recent,
            table: false,
            row_tags: HashMap::new(),
            detail: Detail::Speech,
            visual: Visual::default(),
            travel: Travel::default(),
            thumbnails,
            deleting: Vec::new(),
        };
        this.run_search(cx);
        this.load_visual(cx);
        this.rescan(cx);
        this.watch_changes(cx);
        this
    }

    fn page(&self) -> Page {
        match self.mode {
            Mode::Browse => Page::Browse,
            Mode::Folders => Page::Folders,
            Mode::Review(_) => Page::Review,
            Mode::Settings(_) => Page::Settings,
            Mode::Studio => Page::Studio,
        }
    }

    /// Settings and the Studio show no results, and no key acts on a file that
    /// cannot be seen.
    fn results_page(&self) -> bool {
        !matches!(self.mode, Mode::Settings(_) | Mode::Studio)
    }

    /// The tab the pane shows. Only a video has speech, so any other file shows
    /// its text, and a choice of Speech waits for the next video.
    fn shown_detail(&self) -> Detail {
        match self.selected_hit() {
            Some(hit) if hit.file.kind != Kind::Video => Detail::Text,
            _ => self.detail,
        }
    }

    /// A page tab, or `ctrl-1` to `ctrl-5`.
    fn show_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if page == self.page() {
            return;
        }
        self.record_step(cx);
        self.enter_page(page, None, window, cx);
    }

    /// A click on a tag: the files with that tag, on the Browse page. The
    /// search splits on spaces, so each word of the tag gets its own `tag:`.
    fn search_tag(&mut self, tag: &str, window: &mut Window, cx: &mut Context<Self>) {
        let query = tag
            .split_whitespace()
            .map(|word| format!("tag:{word}"))
            .collect::<Vec<_>>()
            .join(" ");
        self.record_step(cx);
        self.end_likes();
        self.input
            .update(cx, |input, cx| input.set_value(query, window, cx));
        self.enter_page(Page::Browse, None, window, cx);
    }

    /// The results of the query, or the contents of the folder on the Folders
    /// page while nothing is typed. Sets the status line to match.
    fn fetch_results(&mut self, cx: &mut Context<Self>) -> Vec<Entry> {
        // A visual search for an older query must not land on these results.
        self.visual.task = None;
        let text = self.input.read(cx).value().to_string();
        let folders = self.page() == Page::Folders;
        self.listing = text.trim().is_empty();
        let results = if folders && self.listing {
            self.list_folder()
        } else {
            self.fetch_hits(text, self.scope(), cx)
        };
        self.read_row_tags();
        results
    }

    /// The folder a query searches: the open folder on the Folders page.
    fn scope(&self) -> Option<PathBuf> {
        if self.page() == Page::Folders {
            self.folder.clone()
        } else {
            None
        }
    }

    fn read_row_tags(&mut self) {
        self.row_tags = if self.table_shown() {
            tags::by_hash(self.db.conn()).unwrap_or_else(|e| {
                log::error!("reading the tags failed: {e:#}");
                HashMap::new()
            })
        } else {
            HashMap::new()
        };
    }

    /// The text matches now. The visual matches follow, in `visual`.
    fn fetch_hits(
        &mut self,
        text: String,
        within: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Vec<Entry> {
        let request = search::Request::new(text)
            .kinds(self.kinds.clone())
            .within(within)
            .sort(self.browse_sort);
        match search::search(self.db.conn(), &request, &self.config.weights) {
            Ok(hits) => {
                self.status = count(hits.len());
                self.search_visually(request, hits.clone(), cx);
                hits.into_iter().map(Entry::File).collect()
            }
            Err(e) => {
                log::error!("search failed: {e:#}");
                self.status = format!("search failed: {e:#}").into();
                Vec::new()
            }
        }
    }

    /// A new query. Review holds its own results, which a background refresh
    /// must not replace.
    fn run_search(&mut self, cx: &mut Context<Self>) {
        if self.results_page() && self.review().is_none() && !self.search_like(true, cx) {
            let results = self.fetch_results(cx);
            self.show_results(results, cx);
        }
    }

    /// The same query again, after the index changed under it.
    fn rerun_search(&mut self, cx: &mut Context<Self>) {
        if self.results_page() && self.review().is_none() && !self.search_like(false, cx) {
            let results = self.fetch_results(cx);
            self.update_results(results, cx);
        }
    }

    /// New results. Everything that was about the old ones goes with them.
    fn show_results(&mut self, results: Vec<Entry>, cx: &mut Context<Self>) {
        self.results = results;
        self.selected = 0;
        self.stop_videos();
        // A mark is a promise about a file the user can see. New results are a
        // different set of files, so the promise cannot be kept.
        self.marked.clear();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.land();
        self.schedule_preview(cx);
        cx.notify();
    }

    /// The same results with the index changed under them. A tag can move a
    /// file up the ranking, and dropping the user back at the top of the grid
    /// mid-tagging would lose their place.
    fn update_results(&mut self, results: Vec<Entry>, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.results, results);

        // Follow the file, not the row. A tag reorders the results, so the row
        // the selection sat on can now hold something else, and taking the tag
        // that found a file off it drops the file out of the results entirely.
        let last = self.results.len().saturating_sub(1);
        self.selected = old
            .get(self.selected)
            .and_then(|entry| self.position_of(entry))
            .unwrap_or(self.selected.min(last));

        // A video plays for the file it was opened for, and a mark on a file
        // that is no longer a result cannot be seen.
        let results = &self.results;
        self.player.retain(|at| {
            old.get(at)
                .zip(results.get(at))
                .is_some_and(|(was, now)| was.key() == now.key())
        });
        self.marked.retain(|id| {
            results
                .iter()
                .filter_map(Entry::hit)
                .any(|hit| hit.file.id == *id)
        });

        // The selection can land on a different file even when the row number
        // does not change, and the pane would otherwise keep playing the clip
        // that is no longer selected.
        self.schedule_preview(cx);
        cx.notify();
    }

    /// Where `entry` is in the results now: the same row of the database, or
    /// failing that the same bytes, or the same path for anything else.
    fn position_of(&self, entry: &Entry) -> Option<usize> {
        if let Entry::File(hit) = entry
            && let Some(at) = self
                .results
                .iter()
                .position(|now| now.hit().is_some_and(|now| now.file.id == hit.file.id))
        {
            return Some(at);
        }
        let key = entry.key();
        self.results.iter().position(|now| now.key() == key)
    }

    fn hit_at(&self, index: usize) -> Option<&Hit> {
        self.results.get(index).and_then(Entry::hit)
    }

    fn selected_hit(&self) -> Option<&Hit> {
        self.hit_at(self.selected)
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.results.len() || index == self.selected {
            return;
        }
        // One cell or one row is travel, and says what to open ahead. A click
        // across the grid says nothing, so the direction holds.
        let step = index as isize - self.selected as isize;
        if step.abs() <= self.columns.max(1) as isize {
            self.player.step = step;
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

    /// Enter, or a double click. A folder opens; a file goes on the clipboard.
    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.results.get(self.selected) {
            Some(Entry::Folder(path)) => {
                let path = path.clone();
                self.open_folder(Some(path), window, cx);
            }
            _ => self.copy_selected(cx),
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
        self.reload_review(cx);
    }

    /// Every page with results can show the table.
    fn table_shown(&self) -> bool {
        self.table && self.results_page()
    }

    /// `v`, or a view button.
    fn set_table(&mut self, table: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.results_page() || self.table == table {
            return;
        }
        self.table = table;
        // The table has no tile to play in.
        self.player.hovered = None;
        self.rerun_search(cx);
        self.reveal_selection(window, cx);
    }

    /// Scroll to the selection with the columns of the next frame. They change
    /// with the view.
    fn reveal_selection(&mut self, window: &Window, cx: &App) {
        self.columns = self.column_count(window, cx);
        self.scroll
            .scroll_to_item(self.selected / self.columns, ScrollStrategy::Nearest);
    }

    fn column_count(&self, window: &Window, cx: &App) -> usize {
        if self.table_shown() {
            1
        } else {
            grid::columns(self.grid_width(window, cx))
        }
    }

    /// The width the grid had in the last frame, which decides the columns.
    fn grid_width(&self, window: &Window, cx: &App) -> f32 {
        let sizes = self.panes.read(cx).sizes();
        match sizes.first() {
            Some(width) if *width > px(0.) => f32::from(*width),
            _ => f32::from(window.viewport_size().width) - SIDE_PANE_WIDTH,
        }
    }

    fn query_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.read(cx).focus_handle(cx).is_focused(window)
    }

    fn focus_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.page(), Page::Browse | Page::Folders) {
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
    /// opening, so every frame takes it back for the window keys. The Studio
    /// holds its own, so its keys win there.
    fn hold_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let held = if self.page() == Page::Studio && self.studio.edit.is_some() {
            &self.studio.focus
        } else {
            &self.focus
        };
        if !held.is_focused(window)
            && !self.query_focused(window, cx)
            && self.tagger.is_none()
            && self.renamer.is_none()
            && self.rows.is_none()
            && self.settings().is_none()
            && !self.menu_open
        {
            window.focus(&held.clone(), cx);
        }
    }

    /// Escape, wherever it comes from. It undoes one thing at a time, the most
    /// recent first, so a person leaning on it ends up back at a plain grid.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.end_autoscroll(cx) {
            return;
        }
        if self.tagger.is_some() {
            self.close_tagger(window, cx);
        } else if self.renamer.is_some() {
            self.close_renamer(window, cx);
        } else if self.rows.as_ref().is_some_and(|rows| rows.input.is_some()) {
            self.close_row_input(window, cx);
        } else if self.rows.is_some() {
            self.close_rows(window, cx);
        } else if self.settings().is_some() && !self.focus.is_focused(window) {
            // A field of the form has focus. Leaving would drop what was typed.
            window.focus(&self.focus, cx);
        } else if self.page() == Page::Studio {
            // Back where `c` was pressed, with the selection it had.
            if !self.go_history(false, window, cx) {
                self.show_page(Page::Browse, window, cx);
            }
        } else if matches!(self.page(), Page::Review | Page::Settings) {
            self.show_page(Page::Browse, window, cx);
        } else if self.query_focused(window, cx) {
            self.blur_query(window, cx);
        } else if !self.marked.is_empty() {
            self.marked.clear();
            cx.notify();
        } else {
            self.leave_like(window, cx);
        }
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

    /// The selected path, if it is still on disk. The index is a snapshot, so a
    /// file can be gone while its tile is still on screen, and every action
    /// against a missing file fails silently in the platform call.
    fn selected_file(&mut self, cx: &mut Context<Self>) -> Option<PathBuf> {
        let path = self.results.get(self.selected)?.path();
        match dunce::canonicalize(path) {
            Ok(path) => Some(path),
            Err(_) => {
                self.flash("the file is gone", true, cx);
                None
            }
        }
    }

    /// Enter. The file goes on the clipboard as a file and as its path, so the
    /// next paste works in an upload box and in a text field alike.
    fn copy_selected(&mut self, cx: &mut Context<Self>) {
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

    /// The files a bulk action works on: everything marked, or the selection
    /// when nothing is marked. Acting on the selection while marks exist would
    /// be a quiet way to tag the wrong file. Tags and looks belong to the bytes,
    /// so two marked copies of one file are one target.
    fn action_targets(&self) -> Vec<String> {
        if !self.marked.is_empty() {
            let mut hashes: Vec<String> = self
                .results
                .iter()
                .filter_map(Entry::hit)
                .filter(|hit| self.is_marked(hit))
                .map(|hit| hit.file.hash.clone())
                .collect();
            hashes.sort();
            hashes.dedup();
            return hashes;
        }
        self.selected_hit()
            .map(|hit| vec![hit.file.hash.clone()])
            .unwrap_or_default()
    }

    fn toggle_mark(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(hit) = self.hit_at(index) else {
            return;
        };
        let id = hit.file.id;
        if !self.marked.remove(&id) {
            self.marked.insert(id);
        }
        cx.notify();
    }

    fn is_marked(&self, hit: &Hit) -> bool {
        self.marked.contains(&hit.file.id)
    }
}

impl Render for FileBuddy {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        gpui_video::release_unused(window, cx);
        self.sync_rows();
        self.hold_focus(window, cx);
        self.sync_placeholder(window, cx);
        self.sync_seek_bar(Clip::Pane, window, cx);
        self.step_studio(window, cx);
        let width = self.grid_width(window, cx);
        self.columns = self.column_count(window, cx);
        self.step_autoscroll(window, cx);

        v_flex()
            .key_context("FileBuddy")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.move_selection(-1, 0, cx)))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.move_selection(1, 0, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, window, cx| {
                if !this.focus_keys(window, cx) {
                    this.move_selection(0, -1, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &SelectDown, window, cx| {
                if !this.focus_keys(window, cx) {
                    this.move_selection(0, 1, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &KeyRowUp, _, cx| this.move_key_row(false, cx)))
            .on_action(cx.listener(|this, _: &KeyRowDown, _, cx| this.move_key_row(true, cx)))
            .on_action(cx.listener(|this, _: &CaptureKey, _, cx| this.start_capture(cx)))
            .on_action(cx.listener(|this, _: &RemoveKey, _, cx| this.remove_key(cx)))
            .on_action(cx.listener(|this, _: &ResetKeys, _, cx| this.reset_keys(cx)))
            .on_action(cx.listener(|this, _: &FocusQuery, window, cx| this.focus_query(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleMuted, _, cx| {
                this.player.toggle_muted();
                this.sync_studio_sound();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TogglePaused, _, cx| {
                if this.page() == Page::Studio {
                    this.toggle_studio_paused(cx);
                } else {
                    this.press_play(PressFrom::Key, cx);
                }
            }))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                let key = &event.keystroke.key;
                let bound = cx
                    .key_bindings()
                    .borrow()
                    .bindings_for_action(&TogglePaused)
                    .any(|binding| binding.keystrokes().last().is_some_and(|k| k.key() == key));
                if bound {
                    this.release_play(PressFrom::Key, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SkipBack, _, cx| this.skip(false, cx)))
            .on_action(cx.listener(|this, _: &SkipForward, _, cx| this.skip(true, cx)))
            .on_action(cx.listener(|this, _: &ToggleAutoplay, _, cx| {
                this.player.toggle_autoplay();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| {
                this.player.nudge_volume(true);
                this.sync_studio_sound();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| {
                this.player.nudge_volume(false);
                this.sync_studio_sound();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CopyFile, _, cx| this.copy_selected(cx)))
            .on_action(cx.listener(|this, _: &TrashFiles, window, cx| {
                this.delete_files(DeleteMode::Trash, window, cx)
            }))
            .on_action(cx.listener(|this, _: &DeleteFiles, window, cx| {
                this.delete_files(DeleteMode::Delete, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Activate, window, cx| this.activate(window, cx)))
            .on_action(cx.listener(|this, _: &GoUp, window, cx| this.go_up(window, cx)))
            .on_action(cx.listener(|this, _: &CycleSort, window, cx| {
                this.set_sort(this.sort().next(), window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleTable, window, cx| {
                this.set_table(!this.table, window, cx)
            }))
            .on_action(cx.listener(|this, _: &RevealFile, _, cx| {
                if let Some(path) = this.selected_file(cx) {
                    cx.reveal_path(&path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenFile, _, cx| {
                if let Some(path) = this.selected_file(cx) {
                    cx.open_with_system(&path);
                }
            }))
            .on_action(cx.listener(|this, _: &TagFiles, window, cx| this.open_tagger(window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleMark, _, cx| this.toggle_mark(this.selected, cx)),
            )
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.dismiss(window, cx)))
            .on_action(cx.listener(|this, _: &HistoryBack, window, cx| {
                this.go_history(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &HistoryForward, window, cx| {
                this.go_history(true, window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &MoreLikeThis, window, cx| this.more_like_this(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &RenameFile, window, cx| this.open_renamer(window, cx)),
            )
            .on_action(cx.listener(|this, _: &CycleDetail, _, cx| {
                this.detail = this.detail.next();
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &EditRows, window, cx| this.edit_rows(None, window, cx)),
            )
            .on_action(cx.listener(|this, _: &RowUp, _, cx| this.move_row(false, cx)))
            .on_action(cx.listener(|this, _: &RowDown, _, cx| this.move_row(true, cx)))
            .on_action(cx.listener(|this, _: &EditRow, window, cx| this.edit_row(window, cx)))
            .on_action(cx.listener(|this, _: &DeleteRow, _, cx| this.delete_row(cx)))
            .on_action(cx.listener(|this, _: &AddRow, window, cx| this.add_row(window, cx)))
            .on_action(cx.listener(|this, _: &ShowBrowse, window, cx| {
                this.show_page(Page::Browse, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowFolders, window, cx| {
                this.show_page(Page::Folders, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowReview, window, cx| {
                this.show_page(Page::Review, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
                this.show_page(Page::Settings, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowStudio, window, cx| {
                this.show_page(Page::Studio, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &OpenInStudio, window, cx| this.open_in_studio(window, cx)),
            )
            .on_action(cx.listener(|this, _: &SaveSettings, _, cx| this.save_settings(cx)))
            .on_action(cx.listener(|this, _: &Rescan, _, cx| {
                this.flash("scanning the folders", false, cx);
                this.rescan(cx);
            }))
            .on_action(cx.listener(|this, _: &ReviewDuplicates, window, cx| {
                this.show_group(Group::Duplicates, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReviewSameNames, window, cx| {
                this.show_group(Group::SameNames, window, cx)
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
            .child(header::header(self, window, cx))
            .map(|page| match &self.mode {
                Mode::Settings(_) => page.child(settings::page(self, cx)),
                Mode::Studio => page.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(studio::page(self, window, cx)),
                ),
                // The group is as tall as its parent, so it gets a parent of
                // the height under the header, or it runs past the window.
                Mode::Browse | Mode::Folders | Mode::Review(_) => {
                    page.child(
                        div().flex_1().min_h_0().child(
                            h_resizable("panes")
                                .with_state(&self.panes)
                                .child(resizable_panel().size(px(width)).child(
                                    v_flex().size_full().child(header::bar(self, cx)).child(
                                        div().flex_1().min_h_0().child(grid::grid(self, cx)),
                                    ),
                                ))
                                .child(
                                    resizable_panel()
                                        .size(px(SIDE_PANE_WIDTH))
                                        .size_range(px(260.)..px(900.))
                                        .child(preview::pane(self, window, cx)),
                                ),
                        ),
                    )
                }
            })
            .child(tagger::panel(self, cx))
            .child(history::mouse_buttons(cx))
    }
}

/// A button that is on or off, such as a tab or a filter. The caller gives it a
/// size, a tooltip and what a click does. A primary button has no border, so
/// the one that is on gets one, or it would be 2 px narrower than the others.
fn toggle(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Button {
    Button::new(id)
        .label(label)
        .selected(on)
        .when(on, |button| button.primary().border_1())
        .cursor_pointer()
}

/// `1 file` or `3 files`, for a line that says what it touched.
fn counted(files: usize) -> String {
    match files {
        1 => "1 file".to_string(),
        n => format!("{n} files"),
    }
}
