//! The header: the page tabs, what the page puts beside them, the view buttons
//! and the state line at the end.

use gpui::prelude::*;
use gpui::{AnyElement, Context, MouseDownEvent, SharedString, div, px};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};

use crate::media::Kind;
use crate::ui::{FileBuddy, Mode, Page, counted, review, toggle};

/// One height for every page, so the body does not jump when the search box
/// comes and goes.
const HEIGHT: f32 = 58.;

const PAGES: [(Page, &str, &str); 4] = [
    (Page::Browse, "Browse", "Ctrl+1"),
    (Page::Folders, "Folders", "Ctrl+2"),
    (Page::Review, "Review", "Ctrl+3"),
    (Page::Settings, "Settings", "Ctrl+4"),
];

/// The media kind filters. All are on by default, and the last one on cannot
/// be switched off.
const CHIPS: [(Kind, &str); 3] = [
    (Kind::Video, "Video"),
    (Kind::Animated, "GIF"),
    (Kind::Still, "Image"),
];

pub fn header(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let settings = this.settings().is_some();
    h_flex()
        .w_full()
        .h(px(HEIGHT))
        .flex_none()
        .gap_3()
        .px_3()
        .items_center()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(pages(this, cx))
        .map(|header| match &this.mode {
            Mode::Review(state) => header.child(review::tabs(state, cx)),
            Mode::Settings(_) => header.child(div().flex_1()),
            Mode::Browse | Mode::Folders => header
                .child(
                    div()
                        .flex_1()
                        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            this.blur_query(window, cx)
                        }))
                        .child(Input::new(&this.input)),
                )
                .child(chips(this, cx)),
        })
        .when(!settings, |header| header.child(views(this, cx)))
        .when(!this.marked.is_empty(), |header| {
            header.child(
                div()
                    .px_1()
                    .rounded(px(3.))
                    .bg(cx.theme().success.opacity(0.18))
                    .text_color(cx.theme().success)
                    .text_xs()
                    .child(format!("{} marked", this.marked.len())),
            )
        })
        .when(!settings, |header| {
            header.child(
                div()
                    .min_w(px(56.))
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(this.player.volume_label()),
            )
        })
        .when(!this.key_problems.is_empty(), |header| {
            header.child(
                div()
                    .max_w(px(260.))
                    .truncate()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(format!("[keys] skipped: {}", this.key_problems.join("; "))),
            )
        })
        .when_some(this.indexer.progress.as_ref(), |header, progress| {
            header.child(
                div()
                    .max_w(px(320.))
                    .truncate()
                    .text_sm()
                    .text_color(if progress.failed {
                        cx.theme().danger
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(progress.text.clone()),
            )
        })
        .child(match &this.notice {
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
                .child(state(this)),
        })
        .into_any_element()
}

/// The result count, or what the page says in its place.
fn state(this: &FileBuddy) -> SharedString {
    match &this.mode {
        Mode::Review(_) if this.walking() && !this.results.is_empty() => {
            format!("naming {} / {}", this.selected + 1, this.results.len()).into()
        }
        Mode::Review(_) => counted(this.results.len()).into(),
        Mode::Settings(_) => SharedString::default(),
        Mode::Browse | Mode::Folders => this.status.clone(),
    }
}

fn pages(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let current = this.page();
    h_flex()
        .gap_1()
        .children(PAGES.map(|(page, label, key)| {
            toggle(label, label, page == current)
                .small()
                .tooltip(key)
                .on_click(cx.listener(move |this, _, window, cx| this.show_page(page, window, cx)))
        }))
        .into_any_element()
}

fn chips(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .gap_1()
        .children(CHIPS.map(|(kind, label)| {
            toggle(kind.as_str(), label, this.kinds.contains(&kind))
                .xsmall()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_kind(kind, cx)))
        }))
        .into_any_element()
}

/// The Grid and Table buttons.
fn views(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .gap_1()
        .children([("Grid", false), ("Table", true)].map(|(label, table)| {
            toggle(label, label, this.table == table)
                .xsmall()
                .tooltip("V")
                .on_click(cx.listener(move |this, _, window, cx| this.set_table(table, window, cx)))
        }))
        .into_any_element()
}
