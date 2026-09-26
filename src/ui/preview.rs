//! The side pane. It plays the selection whole, with sound if the user asks for
//! it, and shows what the index knows under three tabs: the facts of the file,
//! the speech that was heard and the text that was read.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, MouseButton, ScrollDelta, ScrollWheelEvent,
    SharedString, Stateful, Window, div, img, px,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::DropdownMenu;
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
use crate::ui::player::PressFrom;
use crate::ui::rows::{RowInput, Target};
use crate::ui::{
    CopyFile, Detail, FileBuddy, OpenFile, RenameFile, RevealFile, SIDE_PANE_WIDTH, renamer,
    seek_bar, toggle,
};

/// What the pane holds besides the stage: the controls above it, and the seek
/// bar, the tabs and the panel below. The stage gets the rest.
const CHROME_HEIGHT: f32 = 380. + seek_bar::HEIGHT + 8.;
const DETAIL_HEIGHT: f32 = 200.;
const MIN_STAGE_HEIGHT: f32 = 180.;
const PADDING: f32 = 12.;
/// The Play and Pause button, with room for the longer label.
const PAUSE_WIDTH: f32 = 76.;

const DETAILS: [(Detail, &str); 3] = [
    (Detail::File, "File"),
    (Detail::Speech, "Speech"),
    (Detail::Text, "Text"),
];

pub fn pane(this: &FileBuddy, window: &Window, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(hit) = this.selected_hit() else {
        let text = match this.results.get(this.selected) {
            Some(Entry::Folder(_)) => "A folder. Enter opens it.".to_string(),
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
        .child(controls(this, cx))
        .child(stage(this, hit, width, height, cx))
        .child(seek_bar::bar(this, width, cx))
        .child(tabs(this, cx))
        .child(match this.detail {
            Detail::File => file_details(this, hit, cx),
            Detail::Speech => transcript(this, hit, cx),
            Detail::Text => screen_text(this, hit, cx),
        })
        .into_any_element()
}

/// The three tabs under the clip. `i` cycles them.
fn tabs(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .gap_1()
        .children(DETAILS.map(|(detail, label)| {
            toggle(label, label, detail == this.detail)
                .xsmall()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.detail = detail;
                    cx.notify();
                }))
        }))
        .into_any_element()
}

/// The box a tab fills. Fixed height, so the stage above it never moves, and
/// keyed by the tab and the file, so new contents start at the top.
fn panel(this: &FileBuddy, hit: &Hit, cx: &App) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!(
            "detail-{:?}-{}",
            this.detail, hit.file.hash
        )))
        .w_full()
        .h(px(DETAIL_HEIGHT))
        .flex_shrink_0()
        .overflow_y_scroll()
        .p_2()
        .rounded(px(4.))
        .bg(cx.theme().secondary)
        .text_sm()
}

/// What a tab says when it has nothing to show.
fn note(this: &FileBuddy, hit: &Hit, text: String, cx: &App) -> AnyElement {
    panel(this, hit, cx)
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

/// The name, the facts and the tags of the file. The name is an input while
/// this file is being renamed.
fn file_details(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
    panel(this, hit, cx)
        .child(
            v_flex()
                .w_full()
                .gap_2()
                .child(renamer::name_line(this, hit, cx))
                .child(facts(hit, cx))
                .child(review_line(this, hit, cx))
                .child(tag_line(this, hit, cx)),
        )
        .into_any_element()
}

/// What the recognizer read, line by line, with the time of the first frame
/// that shows it. A line the index leaves out is struck through, so the pane
/// shows what was found and what can be searched.
fn screen_text(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
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
        return note(this, hit, text, cx);
    }
    row_list(this, hit, Table::Ocr, lines, cx)
}

/// The speech of the file, one segment per line. Without segments it says why.
fn transcript(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
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
        return note(this, hit, text, cx);
    }
    row_list(this, hit, Table::Segments, lines, cx)
}

/// The row cursor is on this tab of this file.
fn editing(this: &FileBuddy, hit: &Hit, table: Table) -> bool {
    this.rows
        .as_ref()
        .is_some_and(|rows| rows.hash == hit.file.hash && rows.table == table)
}

/// The rows of a tab, with the cursor and the input while they are edited. A
/// double click on a row edits it.
fn row_list(
    this: &FileBuddy,
    hit: &Hit,
    table: Table,
    rows: Vec<Row>,
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
        let text = match input.filter(|input| current && matches!(input.target, Target::Edit(_))) {
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
        let text = text.id(("row-text", at)).on_click(cx.listener(
            move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 {
                    this.edit_rows(Some(at), window, cx);
                    this.edit_row(window, cx);
                }
            },
        ));
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

    panel(this, hit, cx)
        .when_some(editor, |panel, editor| {
            panel
                .track_focus(&editor.focus)
                .key_context("Rows")
                .track_scroll(&editor.scroll)
        })
        .children(children)
        .into_any_element()
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
/// there. A line with no time keeps the column, so the text lines up.
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
    let time = div()
        .id(id)
        .flex_shrink_0()
        .min_w(px(28.))
        .text_xs()
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
    h_flex()
        .w_full()
        .gap_2()
        .items_start()
        .rounded(px(3.))
        .when(current, |row| row.bg(cx.theme().primary.opacity(0.18)))
        .child(time)
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
/// The wheel over it sets the sound level. A click pauses or plays it, and a
/// press held longer plays it fast until the release.
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
        .rounded(px(4.))
        .bg(cx.theme().secondary)
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.player.video().is_some() {
                    this.press_play(PressFrom::Mouse, cx);
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
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
            let up = match event.delta {
                ScrollDelta::Lines(delta) => delta.y,
                ScrollDelta::Pixels(delta) => f32::from(delta.y),
            };
            if up != 0. {
                this.player.nudge_volume(up > 0.);
                cx.notify();
            }
        }));

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
                    .child(img(thumbnail).max_w(px(width)).max_h(px(height)))
                    .into_any_element()
            } else {
                frame.into_any_element()
            }
        }
    }
}

/// Sound and pause, for the pane only, and the menu of file actions. A tile
/// stays silent whatever these say. The row stays in place with no clip, so the
/// stage below it does not move while a decoder opens.
fn controls(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let player = &this.player;
    let playable = player.video().is_some();
    h_flex()
        .w_full()
        .gap_2()
        .child(
            toggle("preview-autoplay", "Autoplay", player.autoplay)
                .small()
                .tooltip("Ctrl+Space")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.player.toggle_autoplay();
                    cx.notify();
                })),
        )
        .child(
            Button::new("preview-pause")
                .small()
                // As wide as Pause, so the row does not move when it changes.
                .w(px(PAUSE_WIDTH))
                .disabled(!playable)
                .when(playable, |button| button.cursor_pointer())
                .icon(if player.paused {
                    IconName::Play
                } else {
                    IconName::Pause
                })
                .label(if player.paused { "Play" } else { "Pause" })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.player.toggle_paused();
                    cx.notify();
                })),
        )
        .child(
            Button::new("preview-mute")
                .small()
                .disabled(!playable)
                .when(playable, |button| button.cursor_pointer())
                .label(if player.muted { "Unmute" } else { "Mute" })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.player.toggle_muted();
                    cx.notify();
                })),
        )
        .child(
            Button::new("preview-speed")
                .small()
                .cursor_pointer()
                .label(player.speed_label())
                .tooltip("Speed. Hold space or the clip for 2x")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.player.toggle_speed();
                    cx.notify();
                })),
        )
        .child(
            div()
                .min_w(px(56.))
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(player.volume_label()),
        )
        .child(div().flex_1())
        .child(file_menu(this, cx))
        .into_any_element()
}

/// Copy, reveal, rename and open. Each item dispatches the same action as its
/// key, to the window, so the menu shows the key beside it.
fn file_menu(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let focus = this.focus.clone();
    let view = cx.entity().downgrade();
    Button::new("file-menu")
        .small()
        .icon(IconName::Ellipsis)
        .tooltip("File actions")
        .cursor_pointer()
        .dropdown_menu(move |menu, _, _| {
            menu.action_context(focus.clone())
                .menu_with_icon("Copy", IconName::Copy, Box::new(CopyFile))
                .menu_with_icon("Reveal", IconName::FolderOpen, Box::new(RevealFile))
                .menu_with_icon("Rename", IconName::Replace, Box::new(RenameFile))
                .menu_with_icon("Open", IconName::ExternalLink, Box::new(OpenFile))
        })
        // The window takes focus back on every frame, which would close the
        // menu the moment it opened.
        .on_open_change(move |open, _, cx| {
            view.update(cx, |this, _| this.menu_open = *open).ok();
        })
        .into_any_element()
}

fn facts(hit: &Hit, cx: &App) -> AnyElement {
    h_flex()
        .gap_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(facts_text(&hit.file))
        .into_any_element()
}

/// The kind, the length, the size, and the state when it is not ready.
pub(super) fn facts_text(file: &FileRow) -> String {
    let mut parts = vec![file.kind.label().to_string()];
    if let Some(duration) = file.duration {
        parts.push(media::duration(duration));
    }
    parts.push(format!("{:.1} MB", file.size as f64 / 1_048_576.));
    if file.state != State::Ready {
        parts.push(file.state.as_str().to_string());
    }
    parts.join("  ·  ")
}

/// In Review, where the file is, and the whole error of a file that could not
/// be indexed. The tile has room for the first words only.
fn review_line(this: &FileBuddy, hit: &Hit, cx: &App) -> AnyElement {
    let Some(group) = this.review().map(|review| review.group) else {
        return div().into_any_element();
    };
    let folder = hit
        .file
        .path
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    v_flex()
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
        })
        .into_any_element()
}

/// The tags of the file, and the button that opens the tag panel beside them.
fn tag_line(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let names = tags::of_hash(this.db.conn(), &hit.file.hash).unwrap_or_default();
    h_flex()
        .gap_1()
        .flex_wrap()
        .items_center()
        .children(names.into_iter().map(|name| {
            div()
                .px_1()
                .rounded(px(3.))
                .bg(cx.theme().primary.opacity(0.18))
                .text_color(cx.theme().primary)
                .text_xs()
                .child(name)
        }))
        .child(
            Button::new("file-tag")
                .xsmall()
                .icon(IconName::Plus)
                .label("Tag")
                .tooltip("T")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.open_tagger(window, cx))),
        )
        .into_any_element()
}
