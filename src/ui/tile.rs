//! One result. A thumbnail until the pointer arrives, then the clip itself.

use gpui::prelude::*;
use gpui::{AnyElement, App, ClickEvent, Context, Window, div, img, px};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_video::video as video_element;

use super::grid::{TILE_HEIGHT, TILE_WIDTH};
use crate::media::Kind;
use crate::paths;
use crate::search::Signal;
use crate::ui::FileBuddy;

pub fn tile(
    this: &FileBuddy,
    index: usize,
    _window: &mut Window,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let Some(hit) = this.results.get(index) else {
        return div().into_any_element();
    };
    let selected = index == this.selected;
    let marked = this.is_marked(hit);
    let playing = this.hovered.as_ref().filter(|(i, _)| *i == index);

    v_flex()
        .id(("tile", index))
        .w(px(TILE_WIDTH))
        .gap_1()
        .on_hover(
            cx.listener(move |this, hovered: &bool, _, cx| this.set_hovered(index, *hovered, cx)),
        )
        .cursor_pointer()
        // Ctrl marks instead of selecting, the way a file manager does it.
        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            if event.modifiers().control {
                this.toggle_mark(index, cx)
            } else {
                this.select(index, cx)
            }
        }))
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
                .child(thumbnail(this, hit, cx))
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
                })
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
                }),
        )
        .child(
            div()
                .w_full()
                .text_xs()
                .text_color(cx.theme().foreground)
                .truncate()
                .child(hit.file.filename.clone()),
        )
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(badge(hit.signal, cx))
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(hit.matched.clone()),
                ),
        )
        .into_any_element()
}

fn thumbnail(this: &FileBuddy, hit: &crate::search::Hit, cx: &App) -> AnyElement {
    let path = paths::thumbnail_path(&this.cache, &hit.file.hash);
    if path.is_file() {
        return img(path)
            .max_w(px(TILE_WIDTH - 4.))
            .max_h(px(TILE_HEIGHT - 4.))
            .into_any_element();
    }
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(match hit.file.kind {
            Kind::Video => "video",
            Kind::Animated => "animation",
            Kind::Still => "image",
        })
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
