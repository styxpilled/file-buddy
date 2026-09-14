//! The media kind filters. All are on by default, and the last one on cannot be
//! switched off.

use gpui::prelude::*;
use gpui::{AnyElement, Context};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Selectable, Sizable, h_flex};

use crate::media::Kind;
use crate::ui::FileBuddy;

const CHIPS: [(Kind, &str); 3] = [
    (Kind::Video, "Video"),
    (Kind::Animated, "GIF"),
    (Kind::Still, "Image"),
];

pub fn chips(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .gap_1()
        .children(CHIPS.map(|(kind, label)| {
            let on = this.kinds.contains(&kind);
            Button::new(kind.as_str())
                .xsmall()
                .label(label)
                .selected(on)
                .when(on, |button| button.primary())
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_kind(kind, cx)))
        }))
        .into_any_element()
}
