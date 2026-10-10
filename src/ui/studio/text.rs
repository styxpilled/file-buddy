//! The texts of the Studio edit: drawn over the preview, where a press selects
//! one and a drag moves it, and edited in the panel. The rows that time them
//! are in `seek_bar`.

use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, ClickEvent, Context, FontWeight, Hsla, MouseButton, MouseDownEvent, Pixels,
    Point, Render, Window, black, div, px, rgb, white,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};

use std::time::Duration;

use super::{Tab, frame_in};
use crate::studio::{self, Edit, Text};
use crate::ui::seek_bar::{self, Clip};
use crate::ui::{FileBuddy, preview, toggle};

/// Rows of the words input before it scrolls.
const WORDS_ROWS: usize = 4;
/// The padding a textarea gives its words. It has no smaller size.
const TEXTAREA_PX: f32 = 10.;
const TEXTAREA_PY: f32 = 8.;
/// The padding of the words input, as an extra small input has.
const WORDS_INSET: f32 = 4.;
/// The line height of a textarea: 1.25 rem.
const WORDS_LINE: f32 = 20.;

pub(super) struct TextGrab {
    index: usize,
    from: Point<Pixels>,
    start: (f64, f64),
    /// The pointer moved, so the press is a drag and not a tap on the clip.
    dragging: bool,
}

/// A text on the preview, while a drag holds it.
#[derive(Clone, Copy)]
pub(super) struct TextDrag;

impl Render for TextDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// The words input. Enter gives the keys back to the Studio, and
/// `shift-enter` adds a line.
pub(super) fn words_input(
    window: &mut Window,
    cx: &mut Context<FileBuddy>,
) -> (gpui::Entity<TextareaState>, gpui::Subscription) {
    let input = cx.new(|cx| {
        TextareaState::new(window, cx)
            .auto_grow(1, WORDS_ROWS)
            .submit_on_enter(true)
    });
    let events = cx.subscribe_in(
        &input,
        window,
        |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => {
                let words = input.read(cx).value().to_string();
                if let Some(text) = this.selected_text_mut() {
                    text.words = words;
                    cx.notify();
                }
            }
            InputEvent::PressEnter { shift: false, .. } => window.focus(&this.studio.focus, cx),
            _ => {}
        },
    );
    (input, events)
}

impl FileBuddy {
    fn selected_text_mut(&mut self) -> Option<&mut Text> {
        let index = self.studio.selected_text?;
        self.studio.edit.as_mut()?.texts.get_mut(index)
    }

    /// The panel shows the words of the text, or of none.
    pub(in crate::ui) fn select_text(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.studio.selected_text != index {
            self.studio.editing_text = false;
        }
        self.studio.selected_text = index;
        if let Some(index) = index {
            self.studio.tab = Tab::Text;
            self.studio.text_scroll.scroll_to_item(index);
            // A frame and a text are never selected together, so `delete`
            // has one target.
            if !self.studio.strip.selected.is_empty() {
                self.studio.strip.selected.clear();
                self.sync_delay_input(window, cx);
            }
        }
        let words = self
            .selected_text_mut()
            .map(|text| text.words.clone())
            .unwrap_or_default();
        self.studio
            .words_input
            .update(cx, |input, cx| input.set_value(words, window, cx));
        cx.notify();
    }

    /// `n`. The new text's words are edited at once, and selected, so typing
    /// replaces them.
    pub(super) fn add_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.studio.edit else {
            return;
        };
        let index = edit.add_text();
        self.edit_text(index, window, cx);
        self.studio
            .words_input
            .update(cx, |input, cx| input.select_all(window, cx));
    }

    /// A double click on a row of the Text tab: its words become the input.
    fn edit_text(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.select_text(Some(index), window, cx);
        self.studio.editing_text = true;
        self.studio
            .words_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// `delete`, while no input has the keys.
    pub(super) fn delete_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(edit), Some(index)) = (&mut self.studio.edit, self.studio.selected_text) else {
            return;
        };
        let next = edit.remove_text(index);
        self.studio.text_grab = None;
        self.select_text(next, window, cx);
    }

    fn grab_text(
        &mut self,
        index: usize,
        from: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = self
            .studio
            .edit
            .as_ref()
            .and_then(|edit| edit.texts.get(index))
        else {
            return;
        };
        self.studio.text_grab = Some(TextGrab {
            index,
            from,
            start: (text.x, text.y),
            dragging: false,
        });
        if self.studio.selected_text != Some(index) {
            self.select_text(Some(index), window, cx);
        }
        window.focus(&self.studio.focus, cx);
    }

    /// The pointer moves with a text held. The first move ends the press on
    /// the clip, so a drag of the text does not also pause it.
    pub(super) fn drag_text(
        &mut self,
        to: Point<Pixels>,
        stage: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let (Some(edit), Some(grab)) = (&mut self.studio.edit, &mut self.studio.text_grab) else {
            return;
        };
        let size = (f32::from(stage.size.width), f32::from(stage.size.height));
        let Some(frame) = frame_in(edit.size, size) else {
            return;
        };
        let width = edit.crop.width as f32 * frame.scale;
        let height = edit.crop.height as f32 * frame.scale;
        let Some(text) = edit.texts.get_mut(grab.index) else {
            return;
        };
        let dx = f32::from(to.x - grab.from.x) / width;
        let dy = f32::from(to.y - grab.from.y) / height;
        (text.x, text.y) = text.moved(grab.start, f64::from(dx), f64::from(dy));
        let first = !std::mem::replace(&mut grab.dragging, true);
        if first {
            self.cancel_press(Clip::Studio);
        }
        cx.notify();
    }
}

/// The texts over the crop box, in stage pixels. A text shows while the
/// playhead is in its time. The selected text also shows outside it, faint, so
/// it can still move. The box clips them, as the frame edge does in the
/// export.
pub(super) fn layer(
    this: &FileBuddy,
    edit: &Edit,
    (left, top, width, height): (f32, f32, f32, f32),
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let output = edit.output_size();
    let scale = width / output.0.max(1) as f32;
    let at_ms = this.studio_at_ms();
    let accent = Hsla::from(rgb(seek_bar::RANGE_COLOR));
    // A frame much larger than any text, centered on the text's place, centers
    // the text with no measure of it.
    let reach = width.max(height) * 2.;

    let texts = edit.texts.iter().enumerate().filter_map(|(index, text)| {
        let selected = this.studio.selected_text == Some(index);
        let showing = edit.shows(text, at_ms);
        if !(showing || selected) || text.trimmed().is_empty() {
            return None;
        }
        let font_px = text.font_px(output.1);
        let size = font_px as f32 * scale;
        let line = size * studio::LINE_HEIGHT as f32;
        let padding = Text::padding_px(font_px) as f32 * scale;
        let lines = text
            .trimmed()
            .split('\n')
            .map(|line_text| div().h(px(line)).child(line_text.to_string()));
        let block = div()
            .id(("studio-text", index))
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .font_family(studio::FONT_FAMILY)
            .font_weight(FontWeight::BOLD)
            .text_size(px(size))
            .line_height(px(line))
            .text_color(white())
            .whitespace_nowrap()
            .cursor_move()
            .when(text.boxed, |block| block.bg(black()).p(px(padding)))
            .when(!showing, |block| block.opacity(0.5))
            .children(lines)
            .when(selected, |block| {
                block.child(
                    div()
                        .absolute()
                        .top(px(-2.))
                        .left(px(-2.))
                        .right(px(-2.))
                        .bottom(px(-2.))
                        .border_1()
                        .border_color(accent),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    // The stage under it would take the press as a tap.
                    cx.stop_propagation();
                    this.grab_text(index, event.position, window, cx);
                }),
            )
            .on_drag(TextDrag, |drag, _, _, cx| cx.new(|_| *drag));
        let (x, y) = (text.x as f32 * width, text.y as f32 * height);
        Some(
            div()
                .absolute()
                .left(px(x - reach))
                .top(px(y - reach))
                .size(px(reach * 2.))
                .flex()
                .items_center()
                .justify_center()
                .child(block),
        )
    });

    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .overflow_hidden()
        .children(texts)
        .into_any_element()
}

/// The Text tab: one row for each text, as the Speech tab has for each
/// segment, with its time on the first line and its words under it. A click
/// selects a row, and a double click edits its words in place. Under the list,
/// the size and the box of the selected text.
pub(super) fn panel(this: &FileBuddy, edit: &Edit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let theme = cx.theme();
    let (muted, foreground, current) = (
        theme.muted_foreground,
        theme.foreground,
        theme.primary.opacity(0.18),
    );
    let (input_border, input_bg, radius) = (theme.ring, theme.background, theme.radius);
    let selected = this.studio.selected_text;
    let editing = this.studio.editing_text;

    // A click on a time seeks there.
    let time = |id: (&'static str, usize), ms: u64| {
        div()
            .id(id)
            .hover(|time| time.text_color(foreground))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.seek_studio(Duration::from_millis(ms));
                cx.notify();
            }))
            .child(super::point(ms))
    };
    let rows = edit.texts.iter().enumerate().map(|(index, text)| {
        let current_row = selected == Some(index);
        let words = if current_row && editing {
            let lines = this.studio.words_input.read(cx).value().split('\n').count();
            // The textarea has the padding of a full size input, and no size
            // to change it. The box draws the border, and the margins take the
            // padding back, so the words sit where the row shows them.
            div()
                .id(("text-words", index))
                .rounded(radius)
                .border_1()
                .border_color(input_border)
                .bg(input_bg)
                .px(px(WORDS_INSET))
                .overflow_hidden()
                .child(
                    // The textarea takes no margin of its own.
                    div().mx(px(-TEXTAREA_PX)).my(px(-TEXTAREA_PY)).child(
                        Textarea::new(&this.studio.words_input)
                            .appearance(false)
                            .h(px(
                                lines.min(WORDS_ROWS) as f32 * WORDS_LINE + 2. * TEXTAREA_PY
                            )),
                    ),
                )
        } else {
            div().id(("text-words", index)).children(
                text.trimmed()
                    .split('\n')
                    .map(|line| div().child(line.to_string())),
            )
        };
        v_flex()
            .id(("text-row", index))
            .rounded(px(3.))
            .mx(px(-preview::ROW_INSET))
            .px(px(preview::ROW_INSET))
            .py(px(2.))
            .when(current_row, |row| row.bg(current))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 {
                    this.edit_text(index, window, cx);
                } else if this.studio.selected_text != Some(index) {
                    this.select_text(Some(index), window, cx);
                }
            }))
            .when(!edit.still, |row| {
                row.child(
                    h_flex()
                        .gap_1()
                        .text_color(muted)
                        .child(time(("text-start", index), text.start_ms))
                        .child("-")
                        .child(time(("text-end", index), text.end_ms)),
                )
            })
            .child(words)
    });
    let list = div()
        .id("text-list")
        .track_scroll(&this.studio.text_scroll)
        .flex_initial()
        .min_h_0()
        .overflow_y_scroll()
        // Room for the highlight of a row, which reaches past its words, and
        // for the scrollbar, which lies over the rows. As the Speech tab has.
        .pl(px(preview::ROW_INSET))
        .pr(px(12. + preview::ROW_INSET))
        .children(rows)
        .when(edit.texts.is_empty(), |list| {
            list.child(
                div()
                    .text_color(muted)
                    .child("N adds a text at the center of the frame."),
            )
        });
    let add = h_flex().child(div().flex_1()).child(
        Button::new("text-add")
            .xsmall()
            .label("Add")
            .tooltip("N")
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| this.add_text(window, cx))),
    );
    let tab = v_flex().flex_1().min_h_0().gap_2().child(add).child(
        v_flex()
            .relative()
            .mx(px(-preview::ROW_INSET))
            .flex_initial()
            .min_h_0()
            .child(list)
            .vertical_scrollbar(&this.studio.text_scroll),
    );
    let Some(text) = selected.and_then(|index| edit.texts.get(index)) else {
        return tab.into_any_element();
    };
    let (size, boxed) = (text.size, text.boxed);
    let font_px = text.font_px(edit.output_size().1);

    tab.child(
        v_flex()
            .flex_shrink_0()
            .gap_1()
            .child(
                h_flex()
                    .child(div().flex_1().text_color(muted).child("Size"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{font_px} px")),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .children(studio::TEXT_SIZES.into_iter().enumerate().map(
                        |(at, (label, preset))| {
                            toggle(("text-size", at), label, size == preset)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(text) = this.selected_text_mut() {
                                        text.size = preset;
                                        cx.notify();
                                    }
                                }))
                        },
                    )),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        toggle("text-box", "Box", boxed)
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(text) = this.selected_text_mut() {
                                    text.boxed = !text.boxed;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("text-delete")
                            .xsmall()
                            .label("Delete")
                            .tooltip("Delete")
                            .cursor_pointer()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.delete_text(window, cx)),
                            ),
                    ),
            ),
    )
    .into_any_element()
}
