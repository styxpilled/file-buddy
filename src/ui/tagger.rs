//! The tag input. It opens over the grid on the marked files, or on the
//! selection when nothing is marked.
//!
//! Autocomplete is the whole point of it. Nothing stops a person typing
//! `funny`, `Funny` and `funnny` on three different days, and three tags that
//! mean one thing are worse than no tags: every search finds a third of what it
//! should. So the tags that already exist are in front of the user while they
//! type, and the name is lowercased on the way into the database.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, MouseDownEvent, SharedString, Subscription, div, px};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};

use crate::ui::FileBuddy;

/// Suggestions on screen at once. More than this is a list to read, not an
/// answer to a question.
pub const MAX_SUGGESTIONS: usize = 8;

const PANEL_WIDTH: f32 = 460.;

pub struct Tagger {
    pub input: Entity<InputState>,
    /// The files it will tag, taken when it opened. The selection cannot move
    /// while the input holds focus, so this cannot go stale.
    pub targets: Vec<String>,
    /// What the panel says it is about to tag.
    pub subject: SharedString,
    /// Existing tags that match what is typed, best first.
    pub suggestions: Vec<SharedString>,
    /// The suggestion Enter would apply. With none, Enter applies what was
    /// typed, which is how a new tag gets made.
    pub highlighted: Option<usize>,
    /// The tags every target already has, so they can be taken off again.
    pub current: Vec<SharedString>,
    pub _subscriptions: Vec<Subscription>,
}

impl Tagger {
    /// What Enter applies: the highlighted suggestion, or what was typed.
    pub fn chosen(&self, typed: &str) -> Option<String> {
        match self.highlighted.and_then(|at| self.suggestions.get(at)) {
            Some(tag) => Some(tag.to_string()),
            None => {
                let typed = typed.trim();
                (!typed.is_empty()).then(|| typed.to_lowercase())
            }
        }
    }
}

pub fn panel(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(tagger) = &this.tagger else {
        return div().into_any_element();
    };

    div()
        .absolute()
        .top(px(64.))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            v_flex()
                // The panel floats over the grid, so without this a click on a
                // suggestion also lands on the tile behind it, and the tiles
                // under it start playing on hover.
                .occlude()
                .on_mouse_down_out(
                    cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.close_tagger(window, cx)
                    }),
                )
                .w(px(PANEL_WIDTH))
                .gap_2()
                .p_3()
                .rounded(px(6.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .shadow_lg()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tagger.subject.clone()),
                )
                .child(Input::new(&tagger.input))
                .child(suggestions(tagger, cx))
                .child(current(tagger, cx))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("enter applies  ·  alt-up and alt-down pick  ·  escape closes"),
                ),
        )
        .into_any_element()
}

/// The tags that already exist and match. The highlighted one is what Enter
/// applies, so it has to look different from the rest.
fn suggestions(tagger: &Tagger, cx: &mut Context<FileBuddy>) -> AnyElement {
    if tagger.suggestions.is_empty() {
        return div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("no tag like this yet, enter makes it")
            .into_any_element();
    }

    v_flex()
        .gap_px()
        .children(
            tagger
                .suggestions
                .iter()
                .enumerate()
                .map(|(at, name)| {
                    let highlighted = tagger.highlighted == Some(at);
                    let name = name.clone();
                    div()
                        .id(("suggestion", at))
                        .w_full()
                        .px_2()
                        .py_1()
                        .rounded(px(3.))
                        .text_sm()
                        .cursor_pointer()
                        .when(highlighted, |row| {
                            row.bg(cx.theme().accent).text_color(cx.theme().primary)
                        })
                        .child(name.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.apply_tag(name.to_string(), window, cx)
                        }))
                })
                .collect::<Vec<_>>(),
        )
        .into_any_element()
}

/// The tags every target already carries. Clicking one takes it off all of
/// them, which is the only way to undo a typo that got saved.
fn current(tagger: &Tagger, cx: &mut Context<FileBuddy>) -> AnyElement {
    if tagger.current.is_empty() {
        return div().into_any_element();
    }

    h_flex()
        .gap_1()
        .flex_wrap()
        .children(tagger.current.iter().enumerate().map(|(at, name)| {
            let name = name.clone();
            div()
                .id(("current", at))
                .px_1()
                .rounded(px(3.))
                .bg(cx.theme().primary.opacity(0.18))
                .text_color(cx.theme().primary)
                .text_xs()
                .cursor_pointer()
                .child(format!("{name}  ×"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.remove_tag(name.to_string(), window, cx)
                }))
        }))
        .into_any_element()
}
