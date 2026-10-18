//! The side pane. It plays the selection whole, with sound if the user asks for
//! it, and shows what the index knows: the facts and the tags of the file, and
//! under two tabs the speech that was heard and the text that was read.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, MouseButton, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, SharedString, Stateful, Window, div, img, px, relative,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Disableable, IconName, Sizable, h_flex, v_flex};
use gpui_video::video as video_element;

use crate::db::edits::{self, Row, Table};
use crate::db::files::{FileRow, State};
use crate::db::review::Group;
use crate::db::{ocr as ocr_rows, tags};
use crate::index::jobs::{self, Stage};
use crate::media::{self, Kind};
use crate::paths;
use crate::search::Hit;
use crate::search::folder::Entry;
use crate::ui::menu;
use crate::ui::player::PressFrom;
use crate::ui::rows::{RowInput, Target};
use crate::ui::seek_bar::Clip;
use crate::ui::{
    CopyFile, Detail, FileBuddy, OpenFile, RevealFile, SIDE_PANE_WIDTH, renamer, seek_bar, toggle,
    when,
};

/// What the pane keeps for the controls above the stage, and the seek bar, the
/// tabs, the name and the panel below it. The stage gets the rest, and the
/// panel gets what the stage leaves.
const CHROME_HEIGHT: f32 = 380. + seek_bar::HEIGHT + 8.;
const MIN_STAGE_HEIGHT: f32 = 180.;
const PADDING: f32 = 12.;
/// Tighter than the default, so a name on two lines reads as one.
const NAME_LINE_HEIGHT: f32 = 1.4;
/// A tag on the name line.
const TAG_HEIGHT: f32 = 16.;
/// How far the highlight of the row under the cursor reaches past its text.
/// The panel reaches as far past the pane, so the highlight is not cut off.
pub(super) const ROW_INSET: f32 = 4.;
/// The time in front of a row, with room for "10:00".
pub(super) const TIME_WIDTH: f32 = 36.;
/// The Play and Pause button, with room for the longer label.
const PAUSE_WIDTH: f32 = 76.;
/// The row of controls above the clip, in the pane and in the Studio.
pub(super) const CONTROLS_HEIGHT: f32 = 24.;
/// The Mute and Unmute button, with room for the longer label.
const MUTE_WIDTH: f32 = 70.;
/// The volume beside the buttons, with room for "100%".
const VOLUME_WIDTH: f32 = 36.;
/// The Autoplay toggle, the speed button and the file menu, as wide as their
/// content, measured in the window.
const AUTOPLAY_WIDTH: f32 = 74.;
const SPEED_WIDTH: f32 = 32.;
const MENU_WIDTH: f32 = 24.;
const CONTROLS_GAP: f32 = 8.;
/// The narrowest pane that holds the row of controls: the left border, the
/// padding, six items and the spacer, and the gaps between the seven.
pub(super) const MIN_PANE_WIDTH: f32 = 1.
    + PADDING * 2.
    + AUTOPLAY_WIDTH
    + PAUSE_WIDTH
    + MUTE_WIDTH
    + SPEED_WIDTH
    + VOLUME_WIDTH
    + MENU_WIDTH
    + CONTROLS_GAP * 6.;

const DETAILS: [(Detail, &str); 2] = [(Detail::Speech, "Speech"), (Detail::Text, "Text")];

pub fn pane(this: &FileBuddy, window: &mut Window, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(hit) = this.selected_hit() else {
        let text = match this.results.get(this.selected) {
            Some(Entry::Folder(_)) => "A folder. Enter opens it.".to_string(),
            Some(Entry::Dropped) => {
                "The files dropped on the window. Enter opens them.".to_string()
            }
            Some(Entry::Stray { path, why, .. }) => {
                format!("{}: {}", FileRow::name_of(path), why.as_str())
            }
            _ => "No selection".to_string(),
        };
        return v_flex()
            .size_full()
            .p(px(PADDING))
            .items_center()
            .justify_center()
            .border_l_1()
            .border_color(cx.theme().border)
            .text_color(cx.theme().muted_foreground)
            .child(text)
            .into_any_element();
    };

    let (width, height) = stage_size(this, window, cx);

    v_flex()
        .size_full()
        .gap_2()
        .p(px(PADDING))
        .border_l_1()
        .border_color(cx.theme().border)
        .child(controls(this, seek_bar::Clip::Pane, cx))
        .child(stage(this, hit, width, height, cx))
        .child(seek_bar::bar(this, seek_bar::Clip::Pane, width, cx))
        .child(name_and_tags(this, hit, cx))
        .children(review_line(this, hit, cx))
        .child(tabs(this, hit, cx))
        .child(
            div()
                .w_full()
                .h_px()
                .flex_shrink_0()
                .my(px(-2.))
                // increased margin so it looks good outside of review mode
                .when(this.review().is_none(), |bar| bar.mt(px(4.)))
                .bg(cx.theme().border),
        )
        .child(match this.shown_detail() {
            Detail::Speech => transcript(this, hit, window, cx),
            Detail::Text => screen_text(this, hit, window, cx),
        })
        .into_any_element()
}

/// The facts of the file, then the two tabs and the button that opens the tag
/// panel. `i` switches the tab.
fn tabs(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .mr_2()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(facts_text(&hit.file)),
        )
        .children(DETAILS.map(|(detail, label)| {
            let disabled = detail == Detail::Speech && hit.file.kind != Kind::Video;
            toggle(label, label, detail == this.shown_detail())
                .xsmall()
                .disabled(disabled)
                .when(disabled, |tab| tab.cursor_default())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.detail = detail;
                    cx.notify();
                }))
        }))
        .child(
            Button::new("file-tag")
                .xsmall()
                .icon(IconName::Plus)
                .tooltip("T")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.open_tagger(window, cx))),
        )
        .into_any_element()
}

/// The box a tab fills: as tall as its contents, up to the room the stage
/// leaves, and scrolled past that. The stage above it never moves. Keyed by the
/// tab and the file, so new contents start at the top.
fn panel(
    this: &FileBuddy,
    hit: &Hit,
    scroll: Option<&ScrollHandle>,
    content: Stateful<Div>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = SharedString::from(format!(
        "detail-{:?}-{}",
        this.shown_detail(),
        hit.file.hash
    ));
    let scroll = match scroll {
        Some(scroll) => scroll.clone(),
        None => window
            .use_keyed_state(id.clone(), cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone(),
    };
    v_flex()
        .relative()
        .mx(px(-ROW_INSET))
        .flex_initial()
        .min_h_0()
        .child(
            content
                .id(id)
                .track_scroll(&scroll)
                .w_full()
                .flex_initial()
                .min_h_0()
                .overflow_y_scroll()
                .pl(px(ROW_INSET))
                // Room for the scrollbar, which lies over the contents.
                .pr(px(12. + ROW_INSET))
                .text_sm(),
        )
        .vertical_scrollbar(&scroll)
        .into_any_element()
}

/// What a tab says when it has nothing to show.
fn note(
    this: &FileBuddy,
    hit: &Hit,
    text: String,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let content = div()
        .id("note")
        .text_color(cx.theme().muted_foreground)
        .child(text);
    panel(this, hit, None, content, window, cx)
}

/// The name of the file with its tags after it, on the same line as long as
/// they fit. The name is an input while this file is being renamed.
fn name_and_tags(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let line = h_flex()
        .w_full()
        .flex_wrap()
        .items_center()
        .gap_y_1()
        .line_height(relative(NAME_LINE_HEIGHT));
    let line = if let Some(input) = renamer::name_input(this, hit, cx) {
        line.child(input)
    } else {
        // One piece per word, so the tags follow the last line of a long name
        // instead of starting a line of their own.
        line.children(
            name_pieces(&hit.file.filename)
                .into_iter()
                .map(|piece| div().min_w_0().text_sm().child(piece)),
        )
    };
    line.children(tag_line(this, hit, cx)).into_any_element()
}

/// The name split after each space, `_`, `-` and `.`, where a line may break.
fn name_pieces(name: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut piece = String::new();
    for c in name.chars() {
        piece.push(c);
        if matches!(c, ' ' | '_' | '-' | '.') {
            pieces.push(std::mem::take(&mut piece));
        }
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}

/// What the recognizer read, line by line, with the time of the first frame
/// that shows it. A line the index leaves out is struck through, so the pane
/// shows what was found and what can be searched.
fn screen_text(
    this: &FileBuddy,
    hit: &Hit,
    window: &mut Window,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let conn = this.db.conn();
    let lines = edits::rows(conn, Table::Ocr, &hit.file.hash).unwrap_or_default();
    if lines.is_empty() && !editing(this, hit, Table::Ocr) {
        let text = match jobs::state(conn, &hit.file.hash, Stage::Ocr) {
            Ok(Some((state, error))) => match state.as_str() {
                "done" => "No text found".to_string(),
                "skipped" => format!("Not read: {}", error.unwrap_or_default()),
                "failed" => format!("Reading failed: {}", error.unwrap_or_default()),
                _ => "Not read yet".to_string(),
            },
            _ => "Not read yet".to_string(),
        };
        return note(this, hit, text, window, cx);
    }
    row_list(this, hit, Table::Ocr, lines, window, cx)
}

/// The speech of the file, one segment per line. Without segments it says why.
fn transcript(
    this: &FileBuddy,
    hit: &Hit,
    window: &mut Window,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let conn = this.db.conn();
    let lines = edits::rows(conn, Table::Segments, &hit.file.hash).unwrap_or_default();
    if lines.is_empty() && !editing(this, hit, Table::Segments) {
        let text = match jobs::state(conn, &hit.file.hash, Stage::Whisper) {
            Ok(Some((state, error))) => match state.as_str() {
                "done" => "No speech found".to_string(),
                "skipped" => format!("Not transcribed: {}", error.unwrap_or_default()),
                "failed" => format!("Transcription failed: {}", error.unwrap_or_default()),
                _ => "Not transcribed yet".to_string(),
            },
            _ => "No transcript for this kind of file".to_string(),
        };
        return note(this, hit, text, window, cx);
    }
    row_list(this, hit, Table::Segments, lines, window, cx)
}

/// The row cursor is on this tab of this file.
fn editing(this: &FileBuddy, hit: &Hit, table: Table) -> bool {
    this.rows
        .as_ref()
        .is_some_and(|rows| rows.hash == hit.file.hash && rows.table == table)
}

/// The rows of a tab, with the cursor and the input while they are edited. A
/// click on a row puts the cursor on it, and a double click edits it.
fn row_list(
    this: &FileBuddy,
    hit: &Hit,
    table: Table,
    rows: Vec<Row>,
    window: &mut Window,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let editor = this.rows.as_ref().filter(|_| editing(this, hit, table));
    let input = editor.and_then(|editor| editor.input.as_ref());
    let adding = input.and_then(|input| match input.target {
        Target::Add(time) => Some(time),
        Target::Edit(_) => None,
    });
    // A still has one frame, so a time in it says nothing.
    let timed = hit.file.kind != Kind::Still;
    let cursor = editor.map(|editor| editor.cursor);
    let empty = rows.is_empty();

    let mut children = Vec::new();
    for (at, row) in rows.into_iter().enumerate() {
        let current = cursor == Some(at);
        let editing = input.filter(|input| current && matches!(input.target, Target::Edit(_)));
        let text = match editing {
            Some(input) => div().child(Input::new(&input.state).xsmall()),
            None => {
                let dropped = table == Table::Ocr && !ocr_rows::indexed(&row.text, row.edited);
                div()
                    .when(dropped, |text| {
                        text.text_color(cx.theme().muted_foreground).line_through()
                    })
                    .child(row.text)
            }
        };
        // The input of the row being edited takes its own clicks: a double
        // click there selects a word, not the whole row again.
        let text = text.id(("row-text", at)).when(editing.is_none(), |text| {
            text.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.close_row_input(window, cx);
                this.edit_rows(Some(at), window, cx);
                if event.click_count() == 2 {
                    this.edit_row(window, cx);
                }
            }))
        });
        children.push(timed_row(
            this,
            hit,
            ("row-time", at),
            row.time_ms.filter(|_| timed),
            text,
            current,
            cx,
        ));
        if current && let Some(time) = adding {
            children.push(add_line(this, hit, input, time, cx));
        }
    }
    if empty && let Some(time) = adding {
        children.push(add_line(this, hit, input, time, cx));
    }

    let content = div()
        .id("rows")
        .when_some(editor, |content, editor| {
            content.track_focus(&editor.focus).key_context("Rows")
        })
        .children(children);
    let scroll = editor.map(|editor| editor.scroll.clone());
    panel(this, hit, scroll.as_ref(), content, window, cx)
}

/// The input of a new row, with the time it gets.
fn add_line(
    this: &FileBuddy,
    hit: &Hit,
    input: Option<&RowInput>,
    time: Option<i64>,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let text = div()
        .id("row-new")
        .children(input.map(|input| Input::new(&input.state).xsmall()));
    timed_row(this, hit, ("row-time-new", 0), time, text, true, cx)
}

/// A line of a tab with its time in front. A click on the time seeks the clip
/// there. A line with no time keeps the column, so the text lines up. A still
/// has no times at all, so its lines have no column.
fn timed_row(
    this: &FileBuddy,
    hit: &Hit,
    id: (&'static str, usize),
    time_ms: Option<i64>,
    text: Stateful<Div>,
    current: bool,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let playing = this.player.video_of(&hit.file.hash).is_some();
    let timed = hit.file.kind != Kind::Still;
    let time = div()
        .id(id)
        .flex_shrink_0()
        .min_w(px(TIME_WIDTH))
        .text_color(cx.theme().muted_foreground);
    let time = match time_ms {
        Some(ms) => time
            .when(playing, |time| {
                time.cursor_pointer()
                    .hover(|time| time.text_color(cx.theme().foreground))
                    .on_click(cx.listener(move |this, _, _, cx| this.seek_to(ms, cx)))
            })
            .child(media::clock(ms)),
        None => time,
    };
    // No full width: a block child fills its parent, and the negative margin
    // then widens the row, where with a full width it would move it.
    h_flex()
        .gap_2()
        .items_start()
        .rounded(px(3.))
        .when(current, |row| {
            row.mx(px(-ROW_INSET))
                .px(px(ROW_INSET))
                .bg(cx.theme().primary.opacity(0.18))
        })
        .when(timed, |row| row.child(time))
        // Without a zero minimum width a long line pushes past the pane
        // instead of wrapping.
        .child(text.flex_1().min_w_0())
        .into_any_element()
}

/// The box the video fits inside. The element letterboxes the frame in the size
/// it is given, but only when it is given both, so both are passed.
fn stage_size(this: &FileBuddy, window: &Window, cx: &App) -> (f32, f32) {
    let pane_width = this
        .panes
        .read(cx)
        .sizes()
        .get(1)
        .map(|width| f32::from(*width))
        .filter(|width| *width > 0.)
        .unwrap_or(SIDE_PANE_WIDTH);
    let height = f32::from(window.viewport_size().height) - CHROME_HEIGHT;
    (
        (pane_width - PADDING * 2.).max(80.),
        height.max(MIN_STAGE_HEIGHT),
    )
}

/// The clip itself once the selection stops moving, the thumbnail until then.
fn stage(
    this: &FileBuddy,
    hit: &Hit,
    width: f32,
    height: f32,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let frame = div()
        .id("stage")
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .rounded(px(4.))
        .bg(cx.theme().secondary)
        .flex()
        .items_center()
        .justify_center();
    let frame = clip_input(frame, Clip::Pane, cx);

    match this.player.video() {
        Some(playing) => frame
            .child(
                video_element(playing.clone())
                    .id("preview")
                    .size(px(width), px(height))
                    .buffer_capacity(60),
            )
            .into_any_element(),
        None => {
            let thumbnail = paths::thumbnail_path(&this.cache, &hit.file.hash);
            if thumbnail.is_file() {
                frame
                    .child(
                        img(thumbnail)
                            .image_cache(&this.thumbnails)
                            .max_w(px(width))
                            .max_h(px(height)),
                    )
                    .into_any_element()
            } else {
                frame.into_any_element()
            }
        }
    }
}

/// The stage of a clip, in the pane or in the Studio. The wheel over it sets
/// the sound level, and with ctrl it scales the frames in frame mode. A click
/// pauses or plays it, and a press held longer plays it fast until the
/// release.
pub(super) fn clip_input(
    frame: Stateful<Div>,
    clip: Clip,
    cx: &mut Context<FileBuddy>,
) -> Stateful<Div> {
    frame
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                if this.clip_video(clip).is_some() || clip == Clip::Studio && this.framed() {
                    this.press_play(clip, PressFrom::Mouse, cx);
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.release_play(PressFrom::Mouse, cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.release_play(PressFrom::Mouse, cx)),
        )
        .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
            if clip == Clip::Studio && event.modifiers.control && this.framed() {
                return this.wheel_place(event.delta, cx);
            }
            let up = match event.delta {
                ScrollDelta::Lines(delta) => delta.y,
                ScrollDelta::Pixels(delta) => f32::from(delta.y),
            };
            if up != 0. {
                this.nudge_volume(up > 0., cx);
            }
        }))
}

/// Sound, speed and pause, and the menu of file actions. A tile stays silent
/// whatever these say. The row stays in place with no clip, so the stage below
/// it does not move while a decoder opens. Autoplay is for the selection, so
/// the Studio does not show it.
pub(super) fn controls(this: &FileBuddy, clip: Clip, cx: &mut Context<FileBuddy>) -> AnyElement {
    let player = &this.player;
    let transport = this.transport_of(clip);
    let sound = this.clip_video(clip).is_some();
    let playable = sound || clip == Clip::Studio && this.framed();
    let still = clip == Clip::Studio && this.studio.edit.as_ref().is_some_and(|edit| edit.still);
    let row = h_flex()
        .w_full()
        .h(px(CONTROLS_HEIGHT))
        .flex_shrink_0()
        .gap(px(CONTROLS_GAP));
    if still {
        return row
            .child(div().flex_1())
            .child(file_menu(this, clip, cx))
            .into_any_element();
    }
    row.when(clip == Clip::Pane, |row| {
        row.child(
            toggle("preview-autoplay", "Autoplay", player.autoplay)
                .small()
                .w(px(AUTOPLAY_WIDTH))
                .tooltip("Ctrl+Space")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_autoplay(cx))),
        )
    })
    .child(
        Button::new("preview-pause")
            .small()
            // As wide as Pause, so the row does not move when it changes.
            .w(px(PAUSE_WIDTH))
            .disabled(!playable)
            .when(playable, |button| button.cursor_pointer())
            .icon(if transport.paused {
                IconName::Play
            } else {
                IconName::Pause
            })
            .label(if transport.paused { "Play" } else { "Pause" })
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_paused(clip, cx))),
    )
    .child(
        Button::new("preview-mute")
            .small()
            // As wide as Unmute, so the row does not move when it changes.
            .w(px(MUTE_WIDTH))
            .disabled(!sound)
            .when(sound, |button| button.cursor_pointer())
            .label(if player.muted { "Unmute" } else { "Mute" })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_muted(cx))),
    )
    .child(
        Button::new("preview-speed")
            .small()
            .w(px(SPEED_WIDTH))
            .cursor_pointer()
            .label(player.speed_label(transport))
            .tooltip("Hold space for 2x")
            .on_click(cx.listener(|this, _, _, cx| this.toggle_speed(cx))),
    )
    .child(
        div()
            .w(px(VOLUME_WIDTH))
            .flex_none()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .when(player.muted, |volume| volume.line_through())
            .child(player.volume_label()),
    )
    .child(div().flex_1())
    .child(file_menu(this, clip, cx))
    .into_any_element()
}

/// The file actions in the pane. In the Studio the menu acts on the source,
/// and the Studio has no name to rename and does not trash the file it edits.
fn file_menu(this: &FileBuddy, clip: Clip, cx: &mut Context<FileBuddy>) -> AnyElement {
    let focus = match clip {
        Clip::Pane => this.focus.clone(),
        Clip::Studio => this.studio.focus.clone(),
    };
    let view = cx.entity().downgrade();
    let several = view.clone();
    Button::new("file-menu")
        .small()
        .w(px(MENU_WIDTH))
        .icon(IconName::Ellipsis)
        .tooltip("File actions")
        .cursor_pointer()
        .dropdown_menu(move |menu, _, cx| {
            let menu = menu.action_context(focus.clone());
            match clip {
                Clip::Pane => menu::file_items(menu, menu::several(&several, cx)),
                Clip::Studio => menu
                    .menu_with_icon("Copy", menu::icon(IconName::Copy), Box::new(CopyFile))
                    .menu_with_icon(
                        "Reveal",
                        menu::icon(IconName::FolderOpen),
                        Box::new(RevealFile),
                    )
                    .menu_with_icon(
                        "Open",
                        menu::icon(IconName::ExternalLink),
                        Box::new(OpenFile),
                    ),
            }
        })
        // The window takes focus back on every frame, which would close the
        // menu the moment it opened.
        .on_open_change(move |open, _, cx| {
            view.update(cx, |this, _| this.menu_open = *open).ok();
        })
        .into_any_element()
}

/// The kind, the length, the size, how long ago the file changed, and the
/// state when it is not ready.
pub(super) fn facts_text(file: &FileRow) -> String {
    let mut parts = vec![file.kind.label().to_string()];
    if let Some(duration) = file.duration {
        parts.push(media::duration(duration));
    }
    parts.push(media::bytes(file.size));
    parts.push(when::ago_now(file.mtime));
    if file.state != State::Ready {
        parts.push(file.state.as_str().to_string());
    }
    parts.join("  ·  ")
}

/// In Review, where the file is, and the whole error of a file that could not
/// be indexed. The tile has room for the first words only.
fn review_line(this: &FileBuddy, hit: &Hit, cx: &App) -> Option<AnyElement> {
    let group = this.review()?.group;
    let folder = hit
        .file
        .path
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    let line = v_flex()
        .w_full()
        .gap_1()
        .text_xs()
        .child(div().text_color(cx.theme().muted_foreground).child(folder))
        .when(group == Group::Failed, |line| {
            line.child(
                div()
                    .text_color(cx.theme().danger)
                    .child(hit.matched.clone()),
            )
        });
    Some(line.into_any_element())
}

/// The tags of the file. They are items of the name line, so they wrap with
/// it. A click on a tag searches for it, and a right click takes it off the
/// file.
fn tag_line(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> Vec<AnyElement> {
    let names = tags::of_hash(this.db.conn(), &hit.file.hash).unwrap_or_default();
    names
        .into_iter()
        .map(|name| {
            div()
                .id(SharedString::from(format!("file-tag-{name}")))
                .ml_1()
                .mb_neg_1()
                .h(px(TAG_HEIGHT))
                .px_1()
                .rounded(px(3.))
                .bg(cx.theme().primary.opacity(0.18))
                .hover(|tag| tag.bg(cx.theme().primary.opacity(0.3)))
                .cursor_pointer()
                .text_color(cx.theme().primary)
                .text_xs()
                // Lowercase letters sit low in the line box, so centered text
                // looks too low. The tag is a flex item, so this margin cannot
                // pass through it and move the tag instead.
                .child(div().mt(px(-1.)).child(name.clone()))
                .on_click(cx.listener({
                    let name = name.clone();
                    move |this, _, window, cx| this.search_tag(&name, window, cx)
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, _, cx| this.untag_selection(name.clone(), cx)),
                )
                .into_any_element()
        })
        .collect()
}
