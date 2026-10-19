//! The header: the page tabs, what the page puts beside them, the notices and
//! the window buttons. It is the title bar too: Windows draws none, and a drag
//! on the header moves the window. Under it, on every page with results, the
//! bar: the breadcrumb of the Folders page, `All files` on Browse or the groups
//! of Review, the sort, the chips, the view buttons and the count.

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, MouseDownEvent, SharedString, Window, WindowControlArea, div,
    px,
};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex};

use crate::media::Kind;
use crate::ui::{FileBuddy, Mode, Page, counted, folders, review, toggle};

/// One height for every page, so the body does not jump when the search box
/// comes and goes.
pub(super) const HEIGHT: f32 = 58.;
/// Half of what the search box had when it filled the header.
const SEARCH_WIDTH: f32 = 440.;
const WINDOW_BUTTON_SIZE: f32 = 36.;
/// The row of controls over the side pane: its padding, a small button and the
/// gap under it. The bar matches it, so both start their content on one line.
const BAR_HEIGHT: f32 = 12. + 24. + 8.;

const PAGES: [(Page, &str, &str); 5] = [
    (Page::Browse, "Browse", "Ctrl+1"),
    (Page::Folders, "Folders", "Ctrl+2"),
    (Page::Review, "Review", "Ctrl+3"),
    (Page::Studio, "Studio", "Ctrl+4"),
    (Page::Settings, "Settings", "Ctrl+5"),
];

/// The media kind filters. All are on by default, and the last one on cannot
/// be switched off.
const CHIPS: [(Kind, &str); 3] = [
    (Kind::Video, "Video"),
    (Kind::Animated, "GIF"),
    (Kind::Still, "Image"),
];

/// Windows asks which window area is under the pointer and takes the first
/// one it finds, so the drag area would win over a button in it. Every part
/// that takes a click blocks the mouse to the header behind it.
pub fn header(this: &FileBuddy, window: &Window, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .id("header")
        .window_control_area(WindowControlArea::Drag)
        .w_full()
        .h(px(HEIGHT))
        .flex_none()
        .gap_3()
        .pl_2()
        .items_center()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(div().occlude().child(pages(this, cx)))
        .map(|header| match &this.mode {
            Mode::Review(_) | Mode::Settings(_) | Mode::Studio => header,
            Mode::Browse | Mode::Folders => header.child(
                div()
                    .occlude()
                    .w(px(SEARCH_WIDTH))
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.blur_query(window, cx)
                    }))
                    // The X at its right clears the query, for a hand that is on the
                    // mouse only.
                    .child(Input::new(&this.input).cleanable(true)),
            ),
        })
        .child(div().flex_1())
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
        // Held open with no notice, so the search box does not move when one
        // comes.
        .child(
            div()
                .min_w(px(90.))
                .max_w(px(260.))
                .truncate()
                .text_sm()
                .when_some(this.notice.as_ref(), |line, notice| {
                    line.text_color(if notice.failed {
                        cx.theme().danger
                    } else {
                        cx.theme().success
                    })
                    .child(notice.text.clone())
                }),
        )
        .child(window_buttons(window, cx))
        .into_any_element()
}

/// Minimize, maximize or restore, and close. Windows acts on a press in each
/// area itself, as it does on its own title bar, so they need no click
/// handler.
fn window_buttons(window: &Window, cx: &Context<FileBuddy>) -> AnyElement {
    let theme = cx.theme();
    let maximize = if window.is_maximized() {
        IconName::WindowRestore
    } else {
        IconName::WindowMaximize
    };
    let button = |id: &'static str, icon: IconName, area: WindowControlArea, close: bool| {
        let (hover, active) = if close {
            (theme.danger, theme.danger_active)
        } else {
            (theme.secondary_hover, theme.secondary_active)
        };
        div()
            .id(id)
            .occlude()
            .window_control_area(area)
            .size(px(WINDOW_BUTTON_SIZE))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.foreground)
            .hover(|style| style.bg(hover))
            .active(|style| style.bg(active))
            .child(Icon::new(icon).small())
    };
    h_flex()
        .flex_none()
        .gap_1()
        .mr_3()
        .child(button(
            "minimize",
            IconName::WindowMinimize,
            WindowControlArea::Min,
            false,
        ))
        .child(button("maximize", maximize, WindowControlArea::Max, false))
        .child(button(
            "close",
            IconName::WindowClose,
            WindowControlArea::Close,
            true,
        ))
        .into_any_element()
}

/// The bar over the results.
pub fn bar(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .w_full()
        .flex_none()
        .h(px(BAR_HEIGHT))
        .gap_3()
        .px_3()
        .py_2()
        .items_center()
        .text_sm()
        .map(|bar| match &this.mode {
            Mode::Folders => bar.child(folders::breadcrumb(this, cx)),
            Mode::Browse => bar.child("All files"),
            Mode::Review(state) => bar.child(review::tabs(state, cx)),
            Mode::Settings(_) | Mode::Studio => bar,
        })
        .child(div().flex_1())
        .when(matches!(this.page(), Page::Browse | Page::Folders), |bar| {
            bar.child(folders::sort_buttons(this, cx))
        })
        .child(chips(this, cx))
        .child(views(this, cx))
        .child(
            div()
                .min_w(px(90.))
                .text_right()
                .text_color(cx.theme().muted_foreground)
                .child(state(this)),
        )
        .into_any_element()
}

/// The result count, or what the page says in its place.
fn state(this: &FileBuddy) -> SharedString {
    match &this.mode {
        Mode::Review(_) if this.walking() && !this.results.is_empty() => {
            format!("naming {} / {}", this.selected + 1, this.results.len()).into()
        }
        Mode::Review(_) => counted(this.results.len()).into(),
        Mode::Settings(_) | Mode::Studio => SharedString::default(),
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
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.toggle_kind(kind, event.modifiers().control, cx)
                }))
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
