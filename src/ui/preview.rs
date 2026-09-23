//! The side pane. It plays the selection whole, with sound if the user asks for
//! it, and shows what the index knows under three tabs: the facts of the file,
//! the speech that was heard and the text that was read.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Div, ScrollDelta, ScrollWheelEvent, SharedString, Stateful, Window,
    div, img, px,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::{ActiveTheme, Disableable, IconName, Sizable, h_flex, v_flex};
use gpui_video::video as video_element;

use crate::db::files::{FileRow, State};
use crate::db::review::Group;
use crate::db::{ocr as ocr_rows, segments, tags};
use crate::index::jobs::{self, Stage};
use crate::ocr::noise;
use crate::paths;
use crate::search::Hit;
use crate::search::folder::Entry;
use crate::ui::{
    CopyFile, Detail, FileBuddy, OpenFile, RenameFile, RevealFile, SIDE_PANE_WIDTH, renamer, toggle,
};

/// What the pane holds besides the stage: the controls above it, and the tabs
/// and the panel below. The stage gets the rest.
const CHROME_HEIGHT: f32 = 380.;
const DETAIL_HEIGHT: f32 = 200.;
const MIN_STAGE_HEIGHT: f32 = 180.;
const PADDING: f32 = 12.;

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
        .child(tabs(this, cx))
        .child(match this.detail {
            Detail::File => file_details(this, hit, cx),
            Detail::Speech => transcript(this, hit, cx),
            Detail::Text => screen_text(this, hit, cx),
        })
        .into_any_element()
}

/// The three tabs under the clip.
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

/// What the recognizer read, line by line. A line the index leaves out is
/// struck through, so the pane shows what was found and what can be searched.
fn screen_text(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let conn = this.db.conn();
    let lines = ocr_rows::of_hash(conn, &hit.file.hash).unwrap_or_default();
    if lines.is_empty() {
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

    panel(this, hit, cx)
        .children(lines.into_iter().map(|line| {
            let dropped = noise::is_noise(&line);
            div()
                .w_full()
                .when(dropped, |row| {
                    row.text_color(cx.theme().muted_foreground).line_through()
                })
                .child(line)
        }))
        .into_any_element()
}

/// The speech of the file, one segment per line. Without segments it says why.
fn transcript(this: &FileBuddy, hit: &Hit, cx: &App) -> AnyElement {
    let conn = this.db.conn();
    let lines = segments::of_hash(conn, &hit.file.hash).unwrap_or_default();
    if lines.is_empty() {
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

    panel(this, hit, cx)
        .children(lines.into_iter().map(|segment| {
            h_flex()
                .w_full()
                .gap_2()
                .items_start()
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(timestamp(segment.start_ms)),
                )
                // Without a zero minimum width a long line pushes past the pane
                // instead of wrapping.
                .child(div().flex_1().min_w_0().child(segment.text))
        }))
        .into_any_element()
}

/// `0:07` or `1:42`.
fn timestamp(ms: i64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
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
/// The wheel over it sets the sound level.
fn stage(
    this: &FileBuddy,
    hit: &Hit,
    width: f32,
    height: f32,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let frame = div()
        .w(px(width))
        .h(px(height))
        .rounded(px(4.))
        .bg(cx.theme().secondary)
        .flex()
        .items_center()
        .justify_center()
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
    let mut parts = vec![file.kind.as_str().to_string()];
    if let Some(duration) = file.duration {
        parts.push(format!("{duration:.1}s"));
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
