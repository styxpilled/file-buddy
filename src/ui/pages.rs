//! The page tabs at the start of the header: Browse, Review and Settings.

use gpui::prelude::*;
use gpui::{AnyElement, Context};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Selectable, Sizable, h_flex};

use crate::ui::{FileBuddy, Page, with_pointer};

const PAGES: [(Page, &str, &str); 3] = [
    (Page::Browse, "Browse", "Ctrl+1"),
    (Page::Review, "Review", "Ctrl+2"),
    (Page::Settings, "Settings", "Ctrl+3"),
];

pub fn tabs(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let current = this.page();
    h_flex()
        .gap_1()
        .children(PAGES.map(|(page, label, key)| {
            let on = page == current;
            with_pointer(
                Button::new(label)
                    .small()
                    .label(label)
                    .tooltip(key)
                    .selected(on)
                    .when(on, |button| button.primary())
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.show_page(page, window, cx)),
                    ),
                true,
            )
        }))
        .into_any_element()
}
