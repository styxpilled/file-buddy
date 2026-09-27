//! The tag input. It opens over the grid on the marked files, or on the
//! selection when nothing is marked.
//!
//! Autocomplete is the whole point of it. Nothing stops a person typing
//! `funny`, `Funny` and `funnny` on three different days, and three tags that
//! mean one thing are worse than no tags: every search finds a third of what it
//! should. So the tags that already exist are in front of the user while they
//! type, and the name is lowercased on the way into the database.

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, Focusable, MouseDownEvent, SharedString, Subscription, Window,
    div, px,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};

use crate::db::tags;
use crate::ui::{FileBuddy, counted};

/// Suggestions on screen at once. More than this is a list to read, not an
/// answer to a question.
const MAX_SUGGESTIONS: usize = 8;

const PANEL_WIDTH: f32 = 460.;

pub struct Tagger {
    input: Entity<InputState>,
    /// The files it will tag, taken when it opened. The selection cannot move
    /// while the input holds focus, so this cannot go stale.
    targets: Vec<String>,
    /// What the panel says it is about to tag.
    subject: SharedString,
    /// Existing tags that match what is typed, best first.
    suggestions: Vec<SharedString>,
    /// The suggestion Enter would apply. With none, Enter applies what was
    /// typed, which is how a new tag gets made.
    highlighted: Option<usize>,
    /// The tags every target already has, so they can be taken off again.
    current: Vec<SharedString>,
    _subscription: Subscription,
}

impl Tagger {
    /// What Enter applies: the highlighted suggestion, or what was typed.
    fn chosen(&self, typed: &str) -> Option<String> {
        match self.highlighted.and_then(|at| self.suggestions.get(at)) {
            Some(tag) => Some(tag.to_string()),
            None => {
                let typed = typed.trim();
                (!typed.is_empty()).then(|| typed.to_lowercase())
            }
        }
    }
}

impl FileBuddy {
    /// Open the tag input on the marked files, or on the selection.
    pub(super) fn open_tagger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.action_targets();
        if targets.is_empty() {
            return;
        }
        let subject: SharedString = match (self.marked.len(), self.selected_hit()) {
            (0, Some(hit)) => format!("Tag {}", hit.file.filename).into(),
            (1, _) => "Tag 1 marked file".into(),
            (n, _) => format!("Tag {n} marked files").into(),
        };

        let input = cx.new(|cx| InputState::new(window, cx).placeholder("a tag, lowercase"));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.refresh_suggestions(cx),
                InputEvent::PressEnter { .. } => {
                    let typed = input.read(cx).value().to_string();
                    let chosen = this
                        .tagger
                        .as_ref()
                        .and_then(|tagger| tagger.chosen(&typed));
                    if let Some(name) = chosen {
                        this.apply_tag(name, window, cx);
                    }
                }
                _ => {}
            },
        );

        let handle = input.read(cx).focus_handle(cx);
        self.tagger = Some(Tagger {
            input,
            subject,
            current: self.common_tags(&targets),
            targets,
            suggestions: Vec::new(),
            highlighted: None,
            _subscription: subscription,
        });
        self.refresh_suggestions(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    pub(super) fn close_tagger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tagger.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn common_tags(&self, targets: &[String]) -> Vec<SharedString> {
        tags::common_to(self.db.conn(), targets)
            .unwrap_or_else(|e| {
                log::error!("reading the tags failed: {e:#}");
                Vec::new()
            })
            .into_iter()
            .map(SharedString::from)
            .collect()
    }

    /// The existing tags that match what is typed. The first is highlighted, so
    /// a tag that is already there is what Enter applies, not a near miss of it.
    fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        let Some(tagger) = &mut self.tagger else {
            return;
        };
        let typed = tagger.input.read(cx).value().trim().to_lowercase();
        let found = tags::matching(self.db.conn(), &typed, MAX_SUGGESTIONS).unwrap_or_default();
        let exact = found.contains(&typed);
        tagger.suggestions = found.into_iter().map(SharedString::from).collect();
        // Nothing typed is a list to browse, not an answer. A name that is
        // already a tag needs no suggestion either: Enter would do the same.
        tagger.highlighted =
            (!typed.is_empty() && !tagger.suggestions.is_empty() && !exact).then_some(0);
        cx.notify();
    }

    pub(super) fn move_highlight(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(tagger) = &mut self.tagger else {
            return;
        };
        let count = tagger.suggestions.len();
        if count == 0 {
            return;
        }
        // Off either end is what was typed, so a new tag is always reachable.
        tagger.highlighted = match (tagger.highlighted, forward) {
            (None, true) => Some(0),
            (None, false) => Some(count - 1),
            (Some(at), true) if at + 1 < count => Some(at + 1),
            (Some(_), true) => None,
            (Some(0), false) => None,
            (Some(at), false) => Some(at - 1),
        };
        cx.notify();
    }

    /// Apply one tag to every target. The panel stays open with an empty box,
    /// because a file usually wants more than one tag.
    fn apply_tag(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let name = name.trim().to_lowercase();
        let Some(tagger) = &self.tagger else { return };
        if name.is_empty() {
            return;
        }
        let targets = tagger.targets.clone();
        if let Err(e) = tags::attach(self.db.conn_mut(), &targets, &name) {
            log::error!("tagging failed: {e:#}");
            self.flash(format!("tagging failed: {e}"), true, cx);
            return;
        }
        self.flash(
            format!("tagged {} with {name}", counted(targets.len())),
            false,
            cx,
        );
        self.after_tag_change(window, cx);
    }

    fn remove_tag(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tagger) = &self.tagger else { return };
        let targets = tagger.targets.clone();
        if let Err(e) = tags::detach(self.db.conn_mut(), &targets, &name) {
            log::error!("untagging failed: {e:#}");
            self.flash(format!("untagging failed: {e}"), true, cx);
            return;
        }
        self.flash(
            format!("took {name} off {}", counted(targets.len())),
            false,
            cx,
        );
        self.after_tag_change(window, cx);
    }

    /// A right click on a tag in the side pane takes it off the selection.
    pub(super) fn untag_selection(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(hit) = self.selected_hit() else {
            return;
        };
        let hashes = vec![hit.file.hash.clone()];
        if let Err(e) = tags::detach(self.db.conn_mut(), &hashes, &name) {
            log::error!("untagging failed: {e:#}");
            self.flash(format!("untagging failed: {e}"), true, cx);
            return;
        }
        self.flash(format!("took {name} off {}", counted(1)), false, cx);
        if let Some(targets) = self.tagger.as_ref().map(|tagger| tagger.targets.clone()) {
            let current = self.common_tags(&targets);
            if let Some(tagger) = &mut self.tagger {
                tagger.current = current;
            }
        }
        self.rerun_search(cx);
        cx.notify();
    }

    /// The index changed under the results, so run the query again. The panel
    /// keeps its targets and empties its box for the next tag.
    fn after_tag_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tagger) = &self.tagger else { return };
        let input = tagger.input.clone();
        let current = self.common_tags(&tagger.targets);
        input.update(cx, |input, cx| input.set_value("", window, cx));
        if let Some(tagger) = &mut self.tagger {
            tagger.current = current;
        }
        self.rerun_search(cx);
        self.refresh_suggestions(cx);
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
