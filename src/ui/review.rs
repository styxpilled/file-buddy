//! The tabs of the Review screen. They take the place of the search box and the
//! chips while Review is open.

use gpui::prelude::*;
use gpui::{AnyElement, Context};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Selectable, Sizable, h_flex};

use crate::db::review::Group;
use crate::ui::{FileBuddy, Review};

pub fn tabs(review: &Review, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .flex_1()
        .gap_1()
        .children(Group::ALL.into_iter().zip(review.counts).enumerate().map(
            |(at, (group, count))| {
                let on = review.group == group;
                Button::new(group.label())
                    .small()
                    .label(format!("{}  {count}", group.label()))
                    .tooltip(format!("Alt+{}", at + 1))
                    .selected(on)
                    .when(on, |button| button.primary())
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.show_group(group, window, cx)),
                    )
            },
        ))
        .into_any_element()
}
