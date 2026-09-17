//! The rename input. It takes the place of the file name in the side pane, so
//! the name is edited where it is read. The extension shows beside the input
//! and is not part of it.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, SharedString, Subscription, div};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};

use crate::db::files::FileRow;
use crate::search::Hit;
use crate::ui::FileBuddy;

pub struct Renamer {
    pub input: Entity<InputState>,
    /// The file as it was when the input opened.
    pub file: FileRow,
    pub extension: SharedString,
    /// A rename is on its way to disk. A second Enter must not start another.
    pub saving: bool,
    pub _subscriptions: Vec<Subscription>,
}

/// The name line of the side pane: the input when this file is being renamed,
/// the plain name otherwise.
pub fn name_line(this: &FileBuddy, hit: &Hit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(renamer) = this
        .renamer
        .as_ref()
        .filter(|renamer| renamer.file.id == hit.file.id)
    else {
        return div()
            .w_full()
            .text_sm()
            .child(hit.file.filename.clone())
            .into_any_element();
    };

    h_flex()
        .w_full()
        .gap_1()
        .items_center()
        .child(div().flex_1().child(Input::new(&renamer.input).small()))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(renamer.extension.clone()),
        )
        .into_any_element()
}
