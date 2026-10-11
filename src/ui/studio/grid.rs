//! The frame grid: every frame of the strip at once, each with a button that
//! skips it and one that copies it. It takes the room the letterbox leaves: it
//! goes above the stage for a frame wider than the stage, and beside it for a
//! narrower one. `g` shows and hides it. A drag moves the selected frames, as
//! on the strip.

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Div, DragMoveEvent, ElementId, Hsla, MouseButton,
    MouseDownEvent, Render, Stateful, Window, div, px, rgb, uniform_list,
};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};

use super::strip::{self, SKIPPED_OPACITY};
use crate::studio::Edit;
use crate::ui::FileBuddy;
use crate::ui::seek_bar;

pub(super) const GAP: f32 = 8.;
/// The share of the room the grid takes at least and at most.
const MIN_SHARE: f32 = 0.3;
const MAX_SHARE: f32 = 0.6;
const CELL_MIN_WIDTH: f32 = 88.;
const CELL_GAP: f32 = 4.;
/// The room for the scrollbar beside the cells.
const SCROLLBAR: f32 = 12.;
/// The line under each frame, and the buttons under that.
const LABEL_LINE: f32 = 18.;
const BUTTON_LINE: f32 = 22.;

/// Where the stage and the grid go in the room of the stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Placement {
    pub(super) stage: (f32, f32),
    pub(super) grid: (f32, f32),
    pub(super) beside: bool,
}

/// The grid takes the side the letterbox leaves empty, and at least
/// `MIN_SHARE` of the room.
pub(super) fn place(size: (u32, u32), width: f32, height: f32) -> Placement {
    let aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
    let share = |spare: f32, whole: f32| spare.clamp(whole * MIN_SHARE, whole * MAX_SHARE);
    if aspect >= width / height {
        let grid = share(height - width / aspect, height);
        Placement {
            stage: (width, height - grid - GAP),
            grid: (width, grid),
            beside: false,
        }
    } else {
        let grid = share(width - height * aspect, width);
        Placement {
            stage: (width - grid - GAP, height),
            grid: (grid, height),
            beside: true,
        }
    }
}

/// The rows of the grid, which scroll. Only the rows in sight are drawn.
pub(super) fn grid(
    this: &FileBuddy,
    edit: &Edit,
    (width, height): (f32, f32),
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let inner = (width - SCROLLBAR).max(CELL_MIN_WIDTH);
    let columns = (((inner + CELL_GAP) / (CELL_MIN_WIDTH + CELL_GAP)) as usize).max(1);
    let cell_width = (inner - CELL_GAP * (columns - 1) as f32) / columns as f32;
    let aspect = edit.size.0.max(1) as f32 / edit.size.1.max(1) as f32;
    let thumb_height = (cell_width / aspect).clamp(cell_width * 0.5, cell_width * 1.4);
    let rows = edit.frames.len().div_ceil(columns);
    let layout = Layout {
        columns,
        cell_width,
        thumb_height,
    };
    let scroll = this.studio.strip.grid_scroll.clone();
    let len = edit.frames.len();
    let offset = scroll.clone();
    div()
        .id("frame-grid-box")
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .on_drag_move(
            cx.listener(move |this, event: &DragMoveEvent<GridDrag>, _, cx| {
                let top = f32::from(offset.0.borrow().base_handle.offset().y);
                let x = f32::from(event.event.position.x - event.bounds.left());
                let y = f32::from(event.event.position.y - event.bounds.top()) - top;
                this.set_drop_gap(layout.gap_at(x, y, len), cx);
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.drop_frames(window, cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.drop_frames(window, cx)),
        )
        .child(
            uniform_list(
                "frame-grid",
                rows,
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    range.map(|row| grid_row(this, row, layout, cx)).collect()
                }),
            )
            .track_scroll(&scroll)
            .size_full(),
        )
        .vertical_scrollbar(&scroll)
        .into_any_element()
}

#[derive(Clone, Copy)]
struct Layout {
    columns: usize,
    cell_width: f32,
    thumb_height: f32,
}

impl Layout {
    fn row_height(&self) -> f32 {
        self.thumb_height + LABEL_LINE + BUTTON_LINE + CELL_GAP
    }

    /// The gap between frames nearest a point of the grid, from 0 to `len`.
    fn gap_at(&self, x: f32, y: f32, len: usize) -> usize {
        let row = (y / self.row_height()).floor().max(0.) as usize;
        let along = x / (self.cell_width + CELL_GAP);
        let column = ((along + 0.5).floor().max(0.) as usize).min(self.columns);
        (row * self.columns + column).min(len)
    }
}

/// The selected frames, while a drag in the grid holds them. The strip has a
/// drag of its own, because every drag listener hears every drag of its kind.
#[derive(Clone, Copy)]
pub(super) struct GridDrag;

impl Render for GridDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

fn grid_row(
    this: &mut FileBuddy,
    row: usize,
    layout: Layout,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let this: &FileBuddy = this;
    let Some(edit) = this.studio.edit.as_ref() else {
        return div().into_any_element();
    };
    let start = row * layout.columns;
    let end = (start + layout.columns).min(edit.frames.len());
    let cells: Vec<AnyElement> = (start..end)
        .map(|index| cell(this, edit, index, layout, cx))
        .collect();
    h_flex()
        .h(px(layout.row_height()))
        .pb(px(CELL_GAP))
        .gap(px(CELL_GAP))
        .items_start()
        .children(cells)
        .into_any_element()
}

/// A frame with its number and delay, and its Skip and Copy buttons. A click
/// on the picture selects it as a click on the strip does.
fn cell(
    this: &FileBuddy,
    edit: &Edit,
    index: usize,
    layout: Layout,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let frame = &edit.frames[index];
    let strip = &this.studio.strip;
    let selected = strip.selected.contains(&index);
    let current = !frame.skipped && edit.frame_at(strip.at_ms()) == index;
    let theme = cx.theme();
    let accent = Hsla::from(rgb(seek_bar::RANGE_COLOR));
    let (muted, secondary) = (theme.muted_foreground, theme.secondary);
    let colors = ButtonColors {
        idle: theme.secondary,
        hover: theme.secondary_hover,
        on: theme.background,
        on_text: theme.muted_foreground,
        text: theme.secondary_foreground,
    };
    let room = (layout.cell_width - 4., layout.thumb_height - 4.);
    let (first, last) = edit.frame_range;
    let outside = index < first || index > last;
    let len = edit.frames.len();
    let marker = |left: f32| strip::marker(accent, left, layout.thumb_height);
    v_flex()
        .relative()
        .w(px(layout.cell_width))
        .flex_shrink_0()
        .when(strip.drop_gap == Some(index), |cell| {
            cell.child(marker(-CELL_GAP / 2. - 1.))
        })
        .when(strip.drop_gap == Some(len) && index + 1 == len, |cell| {
            cell.child(marker(layout.cell_width + CELL_GAP / 2. - 1.))
        })
        .child(
            div()
                .id(("grid-frame", index))
                .h(px(layout.thumb_height))
                .w_full()
                .rounded(px(3.))
                .overflow_hidden()
                .bg(secondary)
                .border_2()
                .border_color(if selected {
                    accent
                } else {
                    gpui::transparent_black()
                })
                .cursor_pointer()
                .child(
                    div()
                        .size_full()
                        .when(frame.skipped || outside, |thumb| {
                            thumb.opacity(SKIPPED_OPACITY)
                        })
                        .child(strip::thumb(this, edit, frame, room)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.press_frame(index, event.modifiers, window, cx)
                    }),
                )
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.click_frame(index, event, window, cx)
                }))
                .on_drag(GridDrag, |drag, _, _, cx| cx.new(|_| *drag)),
        )
        .child(
            h_flex()
                .h(px(LABEL_LINE))
                .text_xs()
                .text_color(muted)
                .child(div().flex_1().child(format!("{}", index + 1)))
                .child(div().when(current, |label| label.text_color(accent)).child(
                    if frame.skipped {
                        "skip".to_string()
                    } else {
                        format!("{} ms", frame.delay_ms)
                    },
                )),
        )
        .child(
            h_flex()
                .h(px(BUTTON_LINE))
                .gap_1()
                .child(
                    button(("grid-skip", index), "Skip", frame.skipped, colors).on_click(
                        cx.listener(move |this, _, _, cx| this.skip_frames(Some(index), cx)),
                    ),
                )
                .child(
                    button(("grid-copy", index), "Copy", false, colors)
                        .on_click(cx.listener(move |this, _, _, cx| this.copy_frame(index, cx))),
                ),
        )
        .into_any_element()
}

#[derive(Clone, Copy)]
struct ButtonColors {
    idle: Hsla,
    hover: Hsla,
    on: Hsla,
    on_text: Hsla,
    text: Hsla,
}

/// A small button of two elements. The grid draws again on every frame while
/// the frames play, and a kit button is many elements.
fn button(
    id: impl Into<ElementId>,
    label: &'static str,
    on: bool,
    colors: ButtonColors,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .h(px(BUTTON_LINE - 2.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .text_xs()
        .cursor_pointer()
        .bg(if on { colors.on } else { colors.idle })
        .text_color(if on { colors.on_text } else { colors.text })
        .when(!on, |button| button.hover(|style| style.bg(colors.hover)))
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_takes_the_gap_nearest_the_pointer() {
        let layout = Layout {
            columns: 3,
            cell_width: 96.,
            thumb_height: 60.,
        };
        let row = layout.row_height();
        assert_eq!(layout.gap_at(10., 10., 20), 0);
        assert_eq!(layout.gap_at(60., 10., 20), 1);
        assert_eq!(layout.gap_at(299., 10., 20), 3);
        assert_eq!(layout.gap_at(60., row + 5., 20), 4);
        assert_eq!(layout.gap_at(60., row * 9., 20), 20);
        assert_eq!(layout.gap_at(-30., -30., 20), 0);
    }

    #[test]
    fn the_grid_takes_the_side_the_letterbox_leaves() {
        // A 4:3 frame in a 2:1 room leaves the sides empty.
        let side = place((640, 480), 1800., 900.);
        assert!(side.beside);
        assert_eq!(side.grid, (600., 900.));
        assert_eq!(side.stage, (1800. - 600. - GAP, 900.));
        // A tall frame leaves more, up to the most the grid takes.
        assert_eq!(place((480, 854), 1600., 900.).grid.0, 1600. * MAX_SHARE);
        // A frame as wide as the room leaves nothing, and the grid takes the
        // least above it.
        let above = place((1920, 1080), 1600., 900.);
        assert!(!above.beside);
        assert_eq!(above.grid, (1600., 900. * MIN_SHARE));
    }
}
