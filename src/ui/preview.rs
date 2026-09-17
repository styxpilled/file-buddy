//! The side pane. It plays the selection whole, with sound if the user asks for
//! it, and shows what the index knows, the transcript included.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, ScrollDelta, ScrollWheelEvent, SharedString, Window, div, img, px,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Selectable, Sizable, h_flex, v_flex,
};
use gpui_video::video as video_element;

use crate::db::review::Group;
use crate::db::{segments, tags};
use crate::index::jobs::{self, Stage};
use crate::paths;
use crate::search::Hit;
use crate::ui::{FileBuddy, SIDE_PANE_WIDTH, renamer};

/// What the pane holds below the stage: the buttons, the name, the facts, the
/// tags and the transcript. The stage gets the rest of the window.
const CHROME_HEIGHT: f32 = 460.;
const TRANSCRIPT_HEIGHT: f32 = 200.;
const MIN_STAGE_HEIGHT: f32 = 180.;
const PADDING: f32 = 12.;

pub fn pane(this: &FileBuddy, window: &Window, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(hit) = this.selected_hit() else {
        return v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child("No selection")
            .into_any_element();
    };

    let (width, height) = stage_size(this, window, cx);

    v_flex()
        .size_full()
        .gap_2()
        .p(px(PADDING))
        .border_l_1()
        .border_color(cx.theme().border)
        .child(stage(this, hit, width, height, cx))
        .child(controls(this, cx))
        .child(file_actions(cx))
        .child(renamer::name_line(this, hit, cx))
        .child(facts(hit, cx))
        .child(review_line(this, hit, cx))
        .child(tag_line(this, hit, cx))
        .child(transcript(this, hit, cx))
        .into_any_element()
}

/// The speech of the file, one segment per line, in a box of fixed height that
/// scrolls. Without segments it says why.
fn transcript(this: &FileBuddy, hit: &Hit, cx: &App) -> AnyElement {
    let conn = this.db.conn();
    let lines = segments::of_hash(conn, &hit.file.hash).unwrap_or_default();
    let frame = div()
        // Keyed by the file, so a new selection starts at the top.
        .id(SharedString::from(format!("transcript-{}", hit.file.hash)))
        .w_full()
        .h(px(TRANSCRIPT_HEIGHT))
        .flex_shrink_0()
        .overflow_y_scroll()
        .p_2()
        .rounded(px(4.))
        .bg(cx.theme().secondary)
        .text_sm();

    if lines.is_empty() {
        let note = match jobs::state(conn, &hit.file.hash, Stage::Whisper) {
            Ok(Some((state, error))) => match state.as_str() {
                "done" => "No speech found".to_string(),
                "skipped" => format!("Not transcribed: {}", error.unwrap_or_default()),
                "failed" => format!("Transcription failed: {}", error.unwrap_or_default()),
                _ => "Not transcribed yet".to_string(),
            },
            _ => "No transcript for this kind of file".to_string(),
        };
        return frame
            .text_color(cx.theme().muted_foreground)
            .child(note)
            .into_any_element();
    }

    frame
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
                this.nudge_volume(up > 0., cx);
            }
        }));

    match this.preview_video() {
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

/// Sound and pause, for the pane only. A tile stays silent whatever these say,
/// and it plays whatever autoplay says. The row stays in place with no clip, so
/// the pane below it does not move while a decoder opens.
fn controls(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let playable = this.preview_video().is_some();
    h_flex()
        .gap_2()
        .child(
            Button::new("preview-autoplay")
                .small()
                .selected(this.autoplay)
                .label(if this.autoplay {
                    "Autoplay on"
                } else {
                    "Autoplay off"
                })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_autoplay(cx))),
        )
        .child(
            Button::new("preview-pause")
                .small()
                .disabled(!playable)
                .icon(if this.preview_paused {
                    IconName::Play
                } else {
                    IconName::Pause
                })
                .label(if this.preview_paused { "Play" } else { "Pause" })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_preview_paused(cx))),
        )
        .child(
            Button::new("preview-mute")
                .small()
                .disabled(!playable)
                .label(if this.preview_muted { "Unmute" } else { "Mute" })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_preview_muted(cx))),
        )
        .into_any_element()
}

/// What to do with the file. Each one has a key as well, because the point of
/// the application is to copy a clip without reaching for the mouse.
fn file_actions(cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .gap_2()
        .child(
            Button::new("file-copy")
                .small()
                .icon(IconName::Copy)
                .label("Copy")
                .tooltip("Enter")
                .on_click(cx.listener(|this, _, _, cx| this.copy_selected(cx))),
        )
        .child(
            Button::new("file-reveal")
                .small()
                .icon(IconName::FolderOpen)
                .label("Reveal")
                .tooltip("Ctrl+E")
                .on_click(cx.listener(|this, _, _, cx| this.reveal_selected(cx))),
        )
        .child(
            Button::new("file-tag")
                .small()
                .icon(IconName::Plus)
                .label("Tag")
                .tooltip("T")
                .on_click(cx.listener(|this, _, window, cx| this.open_tagger(window, cx))),
        )
        .child(
            Button::new("file-rename")
                .small()
                .icon(IconName::Replace)
                .label("Rename")
                .tooltip("R")
                .on_click(cx.listener(|this, _, window, cx| this.open_renamer(window, cx))),
        )
        .child(
            Button::new("file-open")
                .small()
                .icon(IconName::ExternalLink)
                .label("Open")
                .tooltip("Ctrl+O")
                .on_click(cx.listener(|this, _, _, cx| this.open_selected(cx))),
        )
        .into_any_element()
}

fn facts(hit: &Hit, cx: &App) -> AnyElement {
    let mut parts = vec![hit.file.kind.as_str().to_string()];
    if let Some(duration) = hit.file.duration {
        parts.push(format!("{duration:.1}s"));
    }
    parts.push(format!("{:.1} MB", hit.file.size as f64 / 1_048_576.));
    if hit.file.state != "ready" {
        parts.push(hit.file.state.clone());
    }
    h_flex()
        .gap_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(parts.join("  ·  "))
        .into_any_element()
}

/// In Review, where the file is, and the whole error of a file that could not
/// be indexed. The tile has room for the first words only.
fn review_line(this: &FileBuddy, hit: &Hit, cx: &App) -> AnyElement {
    let Some(group) = this.review_group() else {
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

fn tag_line(this: &FileBuddy, hit: &Hit, cx: &App) -> AnyElement {
    let names = tags::of_hash(this.db.conn(), &hit.file.hash).unwrap_or_default();
    if names.is_empty() {
        return div().into_any_element();
    }
    h_flex()
        .gap_1()
        .flex_wrap()
        .children(names.into_iter().map(|name| {
            div()
                .px_1()
                .rounded(px(3.))
                .bg(cx.theme().primary.opacity(0.18))
                .text_color(cx.theme().primary)
                .text_xs()
                .child(name)
        }))
        .into_any_element()
}
