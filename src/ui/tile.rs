//! One result. A thumbnail until the pointer arrives, then the clip itself. On
//! the Folders page a tile can also be a folder, or a file the search cannot
//! find, coloured by why.

use gpui::prelude::*;
use gpui::{AnyElement, App, ClickEvent, Context, Hsla, Window, div, img, px};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex};
use gpui_video::video as video_element;

use super::folders::DROPPED;
use super::grid::{TILE_HEIGHT, TILE_WIDTH};
use super::{menu, preview};
use crate::db::files::{FileRow, State};
use crate::paths;
use crate::search::folder::{Entry, Stray};
use crate::search::{Hit, Signal};
use crate::ui::FileBuddy;

pub fn tile(this: &FileBuddy, index: usize, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(entry) = this.results.get(index) else {
        return div().into_any_element();
    };
    let selected = index == this.selected;
    let marked = entry.hit().is_some_and(|hit| this.is_marked(hit));
    let playing = this.player.hovered.as_ref().filter(|(at, _)| *at == index);

    let (picture, name, name_color, line) = match entry {
        Entry::File(hit) => {
            let pending = hit.file.state == State::Pending;
            (
                thumbnail(this, hit, cx),
                hit.file.filename.clone(),
                if pending {
                    cx.theme().warning
                } else {
                    cx.theme().foreground
                },
                file_line(this, hit, pending, cx),
            )
        }
        Entry::Folder(path) => (
            Icon::new(IconName::Folder)
                .large()
                .text_color(cx.theme().muted_foreground)
                .into_any_element(),
            this.folder_name(path),
            cx.theme().foreground,
            div().into_any_element(),
        ),
        Entry::Dropped => (
            Icon::new(IconName::Inbox)
                .large()
                .text_color(cx.theme().muted_foreground)
                .into_any_element(),
            DROPPED.to_string(),
            cx.theme().foreground,
            div().into_any_element(),
        ),
        Entry::Stray { path, why, .. } => {
            let color = stray_color(*why, cx);
            (
                placeholder(
                    &path
                        .extension()
                        .map(|extension| extension.to_string_lossy().to_lowercase())
                        .unwrap_or_default(),
                    cx,
                ),
                FileRow::name_of(path),
                color,
                note(why.as_str(), color),
            )
        }
    };

    let tile = v_flex()
        .id(("tile", index))
        .w(px(TILE_WIDTH))
        .gap_1()
        .on_hover(
            cx.listener(move |this, hovered: &bool, _, cx| this.set_hovered(index, *hovered, cx)),
        )
        .cursor_pointer()
        .on_click(cx.listener(move |this, event, window, cx| click(this, index, event, window, cx)))
        .child(
            div()
                .relative()
                .w(px(TILE_WIDTH))
                .h(px(TILE_HEIGHT))
                .overflow_hidden()
                .rounded(px(4.))
                .border_2()
                .border_color(match (selected, marked) {
                    (true, _) => cx.theme().primary,
                    (false, true) => cx.theme().success,
                    (false, false) => cx.theme().border,
                })
                .bg(cx.theme().secondary)
                .flex()
                .items_center()
                .justify_center()
                // The thumbnail stays behind the clip. The element paints
                // nothing until the decoder presents a frame, so dropping the
                // thumbnail on hover left the tile empty for a moment.
                .child(picture)
                .when_some(playing, |container, (_, playing)| {
                    container.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                video_element(playing.clone())
                                    .id(("video", index))
                                    .size(px(TILE_WIDTH - 4.), px(TILE_HEIGHT - 4.))
                                    .buffer_capacity(30),
                            ),
                    )
                })
                // Over the clip, or the clip would hide it.
                .when(marked, |container| {
                    container.child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .right(px(4.))
                            .size(px(16.))
                            .rounded(px(8.))
                            .bg(cx.theme().success)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(cx.theme().background)
                            .child("✓"),
                    )
                }),
        )
        .child(
            div()
                .w_full()
                .text_xs()
                .text_color(name_color)
                .truncate()
                .child(name),
        )
        .child(line);
    menu::on_entry(tile, index, cx).into_any_element()
}

/// A click on a tile or a table row. Ctrl marks instead of selecting, the way a
/// file manager does it.
pub(super) fn click(
    this: &mut FileBuddy,
    index: usize,
    event: &ClickEvent,
    window: &mut Window,
    cx: &mut Context<FileBuddy>,
) {
    if event.modifiers().control {
        this.toggle_mark(index, cx)
    } else {
        this.select(index, cx);
        this.renamer_follows(window, cx);
        if event.click_count() == 2 {
            match this.results.get(index) {
                Some(Entry::Folder(_) | Entry::Dropped) => this.activate(window, cx),
                Some(Entry::File(_)) => this.open_in_studio(window, cx),
                _ => {}
            }
        }
    }
}

/// The second line of a file. With nothing typed it is the facts of the file; in
/// Review it is why the file is there; after a query it is what matched.
fn file_line(this: &FileBuddy, hit: &Hit, pending: bool, cx: &App) -> AnyElement {
    if pending {
        return note("not probed yet", cx.theme().warning);
    }
    if this.listing {
        return note(&preview::facts_text(&hit.file), cx.theme().muted_foreground);
    }
    h_flex()
        .w_full()
        .gap_1()
        .items_center()
        // In Review the second line is not a match, so it has no signal.
        .when(this.review().is_none(), |line| {
            line.child(badge(hit.signal, cx))
        })
        .child(
            div()
                .flex_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(hit.snippet()),
        )
        .into_any_element()
}

fn note(text: &str, color: Hsla) -> AnyElement {
    div()
        .w_full()
        .text_xs()
        .text_color(color)
        .truncate()
        .child(text.to_string())
        .into_any_element()
}

pub(super) fn stray_color(why: Stray, cx: &App) -> Hsla {
    match why {
        Stray::Unindexed => cx.theme().warning,
        Stray::Ignored => cx.theme().danger,
        Stray::Unmatched => cx.theme().muted_foreground,
    }
}

fn thumbnail(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
    if !paths::thumbnail_path(&this.cache, &hit.file.hash).is_file() {
        return placeholder(hit.file.kind.label(), cx);
    }
    let tile = this.thumbnails.update(cx, |thumbnails, cx| {
        thumbnails.tile(&hit.file.hash, &this.cache, cx)
    });
    match tile {
        Some(path) => img(path)
            .image_cache(&this.thumbnails)
            .max_w(px(TILE_WIDTH - 4.))
            .max_h(px(TILE_HEIGHT - 4.))
            .into_any_element(),
        None => div().into_any_element(),
    }
}

fn placeholder(text: &str, cx: &App) -> AnyElement {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_string())
        .into_any_element()
}

/// The signal that put this result where it is. It is the only way to judge the
/// weights while using the application.
fn badge(signal: Signal, cx: &App) -> AnyElement {
    let color = match signal {
        Signal::Tags => cx.theme().primary,
        Signal::Filename => cx.theme().info,
        Signal::Speech => cx.theme().success,
        Signal::Ocr => cx.theme().warning,
        Signal::Visual => cx.theme().chart_5,
    };
    div()
        .px_1()
        .rounded(px(3.))
        .bg(color.opacity(0.18))
        .text_color(color)
        .text_xs()
        .child(signal.as_str())
        .into_any_element()
}
