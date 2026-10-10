//! The progress bar under the clip in the side pane and in the Studio, with the
//! time beside it. A click seeks, a drag scrubs, and the pointer over the bar
//! shows the time under it. `shift-left` and `shift-right` skip, and a time in
//! the Speech tab seeks. In the Studio the bar also holds the range, with a
//! handle at each end, and a row under it for each text, from its start to its
//! end. In frame mode the bar runs on the playhead of the strip, with no
//! range.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, DragMoveEvent, Entity, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Render, Stateful, Subscription, Window, div, px, relative, rgb,
};
use gpui_kit::base::slider::{SliderEvent, SliderState};
use gpui_kit::base::{Slider, SliderIndicator, SliderTrack};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_video::Video;

use crate::media;
use crate::studio::Edge;
use crate::ui::{FileBuddy, Page};

/// How far `shift-left` and `shift-right` move.
const SKIP: Duration = Duration::from_secs(5);
pub(super) const HEIGHT: f32 = 16.;
const HANDLE_WIDTH: f32 = 10.;
const TEXT_ROW_HEIGHT: f32 = 18.;
const TEXT_ROW_GAP: f32 = 3.;
/// The ends of a text's bar, inside it.
const TEXT_END_WIDTH: f32 = 5.;
/// How near the playhead, in pixels, an end of a text's bar snaps to it.
const SNAP: f32 = 6.;
pub(super) const RANGE_COLOR: u32 = 0x30a7fe;

/// The clip a bar plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Clip {
    Pane,
    Studio,
}

pub(super) struct SeekBar {
    /// The share of the clip played, from 0 to 1.
    state: Entity<SliderState>,
    /// Where a drag holds the bar. The clip catches up at its own pace.
    pub(super) dragging: Option<f32>,
    /// The pointer over the bar: its share of the clip, and its distance from
    /// the start of the bar.
    hover: Option<(f32, Pixels)>,
    /// Where a drag wants the clip, while the seek before it still decodes.
    /// Only the latest place counts.
    pending: Option<Duration>,
    _events: Subscription,
}

impl SeekBar {
    pub(super) fn new(clip: Clip, cx: &mut Context<FileBuddy>) -> Self {
        let state = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.0001));
        let events = cx.subscribe(&state, move |this, _, event: &SliderEvent, cx| {
            this.scrub(clip, event, cx);
        });
        Self {
            state,
            dragging: None,
            hover: None,
            pending: None,
            _events: events,
        }
    }
}

/// A handle of the range, while a drag holds it.
#[derive(Clone, Copy)]
pub(super) struct HandleDrag(Edge);

impl Render for HandleDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// An end of a text's bar, while a drag holds it.
#[derive(Clone, Copy)]
pub(super) struct TextEdgeDrag(usize, Edge);

impl Render for TextEdgeDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// The middle of a text's bar, while a drag holds it.
#[derive(Clone, Copy)]
pub(super) struct TextBarDrag;

impl Render for TextBarDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// The rows of the texts the timeline shows. More scroll.
const TEXT_ROWS_SHOWN: f32 = 5.;
/// The room under the bar for the rows of the texts, whether or not there are
/// texts, so the stage keeps its size.
pub(super) const TEXT_ROWS_HEIGHT: f32 =
    TEXT_ROW_GAP + TEXT_ROWS_SHOWN * (TEXT_ROW_HEIGHT + TEXT_ROW_GAP);

fn time_at(length: Duration, share: f32) -> Duration {
    length.mul_f32(share.clamp(0., 1.))
}

impl FileBuddy {
    /// A drag pauses the clip until the release. A clip that plays on while
    /// the bar moves fights every seek.
    fn hold(&mut self, clip: Clip) {
        let Some(video) = self.clip_video(clip).cloned() else {
            return;
        };
        let transport = self.transport(clip);
        if !transport.held {
            transport.held = true;
            video.set_paused(true);
        }
    }

    /// The release seeks to the exact place first, so a clip at its end does
    /// not restart at zero. Then the clip plays or stays paused as the user
    /// chose, also with a choice made during the drag.
    fn let_go(&mut self, clip: Clip, to: Duration) {
        self.bar_of(clip).pending = None;
        match self.clip_video(clip) {
            Some(video) => {
                video.seek(to, true).ok();
            }
            None => self.seek_strip(to),
        }
        if std::mem::take(&mut self.transport(clip).held) {
            self.sync_paused(clip);
        }
    }

    /// A drag seeks to the exact frame, one seek at a time, so a clip that
    /// decodes fast follows the pointer closely and a slow one does not fall
    /// behind it.
    fn scrub_to(&mut self, clip: Clip, to: Duration) {
        if self.clip_video(clip).is_none() {
            self.seek_strip(to);
            return;
        }
        self.hold(clip);
        self.bar_of(clip).pending = Some(to);
        self.send_pending(clip);
    }

    fn send_pending(&mut self, clip: Clip) {
        let Some(video) = self.clip_video(clip).cloned() else {
            return;
        };
        if video.seeking() {
            return;
        }
        if let Some(to) = self.bar_of(clip).pending.take() {
            video.seek(to, true).ok();
        }
    }

    pub(super) fn clip_video(&self, clip: Clip) -> Option<&Video> {
        match clip {
            Clip::Pane => self.player.video(),
            Clip::Studio => self.studio.video.as_ref(),
        }
    }

    /// Where the clip is, and how long it is. In frame mode the Studio has no
    /// video, and the playhead runs on the strip.
    pub(super) fn clip_times(&self, clip: Clip) -> Option<(Duration, Duration)> {
        if let Some(video) = self.clip_video(clip) {
            return Some((video.position(), video.duration()));
        }
        match (clip, &self.studio.edit) {
            (Clip::Studio, Some(edit)) if edit.framed() => Some((
                Duration::from_millis(self.studio.strip.at_ms()),
                Duration::from_millis(edit.duration_ms),
            )),
            _ => None,
        }
    }

    fn bar_of(&mut self, clip: Clip) -> &mut SeekBar {
        match clip {
            Clip::Pane => &mut self.seek_bar,
            Clip::Studio => &mut self.studio.seek_bar,
        }
    }

    /// The clip the page shows, which the skip keys move.
    pub(super) fn shown_clip(&self) -> Clip {
        if self.page() == Page::Studio {
            Clip::Studio
        } else {
            Clip::Pane
        }
    }

    fn scrub(&mut self, clip: Clip, event: &SliderEvent, cx: &mut Context<Self>) {
        let Some((_, length)) = self.clip_times(clip) else {
            return;
        };
        match event {
            SliderEvent::Change(value) => {
                self.bar_of(clip).dragging = Some(value.end());
                self.scrub_to(clip, time_at(length, value.end()));
            }
            SliderEvent::Release(value) => {
                self.bar_of(clip).dragging = None;
                self.let_go(clip, time_at(length, value.end()));
            }
        }
        cx.notify();
    }

    /// A drag on a handle moves it, and the clip follows it.
    fn drag_handle(&mut self, edge: Edge, share: f32, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.studio.edit else {
            return;
        };
        let at = (edit.duration_ms as f32 * share.clamp(0., 1.)) as u64;
        edit.drag(edge, at);
        let at = edit.range.at(edge);
        self.studio.handle = Some(edge);
        self.scrub_to(Clip::Studio, Duration::from_millis(at));
        cx.notify();
    }

    fn release_handle(&mut self, cx: &mut Context<Self>) {
        self.studio.text_move = None;
        let Some(edit) = &self.studio.edit else {
            return;
        };
        let Some(edge) = self.studio.handle.take() else {
            return;
        };
        self.let_go(Clip::Studio, Duration::from_millis(edit.range.at(edge)));
        cx.notify();
    }

    /// A drag on an end of a text's bar. The clip stays where it is, and the
    /// end snaps to the playhead near it. `snap` is a share of the bar.
    fn drag_text_edge(
        &mut self,
        index: usize,
        edge: Edge,
        share: f32,
        snap: f32,
        cx: &mut Context<Self>,
    ) {
        let playhead = self.clip_times(Clip::Studio).map(|(at, _)| at);
        let Some(edit) = &mut self.studio.edit else {
            return;
        };
        let length = edit.duration_ms as f32;
        let mut at = (length * share.clamp(0., 1.)) as u64;
        if let Some(playhead) = playhead {
            let playhead = playhead.as_millis() as u64;
            if (at as f32 - playhead as f32).abs() <= snap * length {
                at = playhead;
            }
        }
        edit.drag_text_edge(index, edge, at);
        cx.notify();
    }

    /// A drag on the middle of a text's bar moves it through time, with no
    /// seek.
    fn drag_text_bar(&mut self, x: Pixels, width: Pixels, cx: &mut Context<Self>) {
        let (Some(edit), Some((index, from, span))) =
            (&mut self.studio.edit, self.studio.text_move)
        else {
            return;
        };
        let delta = ((x - from) / width * edit.duration_ms as f32) as i64;
        edit.move_text(index, span, delta);
        cx.notify();
    }

    /// `shift-left` and `shift-right`. A skip stops at either end of the clip.
    pub(super) fn skip(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(video) = self.clip_video(self.shown_clip()) else {
            return;
        };
        let at = video.position();
        let to = if forward {
            (at + SKIP).min(video.duration())
        } else {
            at.saturating_sub(SKIP)
        };
        video.seek(to, true).ok();
        cx.notify();
    }

    /// A click on a time in the pane.
    pub(super) fn seek_to(&mut self, ms: i64, cx: &mut Context<Self>) {
        if let Some(video) = self.player.video() {
            video
                .seek(Duration::from_millis(ms.max(0) as u64), true)
                .ok();
            cx.notify();
        }
    }

    /// Move the bar to where the clip is. Only a change is written, because a
    /// write redraws the window, and a paused clip must not redraw forever.
    /// A drag sends its next seek from here, once the last one has its frame.
    pub(super) fn sync_seek_bar(
        &mut self,
        clip: Clip,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.send_pending(clip);
        if self.bar_of(clip).dragging.is_some() {
            return;
        }
        let Some((position, length)) = self.clip_times(clip) else {
            return;
        };
        let length = length.as_secs_f32();
        if length <= 0. {
            return;
        }
        let share = (position.as_secs_f32() / length).clamp(0., 1.);
        let state = self.bar_of(clip).state.clone();
        if (state.read(cx).value().end() - share).abs() > 1e-4 {
            state.update(cx, |state, cx| state.set_value(share, window, cx));
        }
    }
}

/// The bar and the time, or the empty track alone when the pane has no clip, so
/// the stage above it does not move and a still reads as a clip with no time. It never shrinks: the empty
/// row has no content to hold it, and a long panel below would squeeze it.
pub(super) fn bar(
    this: &FileBuddy,
    clip: Clip,
    width: f32,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let row = h_flex()
        .w(px(width))
        .h(px(HEIGHT))
        .flex_shrink_0()
        .gap_2()
        .items_center();
    let Some((_, length)) = this.clip_times(clip) else {
        return row
            .child(
                div()
                    .flex_1()
                    .h(px(3.))
                    .rounded(px(2.))
                    .bg(cx.theme().muted),
            )
            .into_any_element();
    };
    let seek_bar = match clip {
        Clip::Pane => &this.seek_bar,
        Clip::Studio => &this.studio.seek_bar,
    };
    let state = &seek_bar.state;
    let share = seek_bar
        .dragging
        .unwrap_or_else(|| state.read(cx).value().end());
    // In the Studio, the bar fills from the in point.
    let filled_from = match (clip, &this.studio.edit) {
        (Clip::Studio, Some(edit)) if !edit.framed() => {
            edit.range.start_ms as f32 / edit.duration_ms.max(1) as f32
        }
        _ => 0.,
    };
    let theme = cx.theme();
    let (popover, border, muted_text) = (theme.popover, theme.border, theme.muted_foreground);
    let time = format!("{} / {}", clock(time_at(length, share)), clock(length));

    row.child(
        div()
            .id("seek-bar")
            .relative()
            .flex_1()
            .h_full()
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let bar = this.bar_of(clip);
                let bounds = bar.state.read(cx).bounds();
                if bounds.size.width <= px(0.) {
                    return;
                }
                let offset = (event.position.x - bounds.left()).clamp(px(0.), bounds.size.width);
                bar.hover = Some((offset / bounds.size.width, offset));
                cx.notify();
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if !hovered {
                    this.bar_of(clip).hover = None;
                    cx.notify();
                }
            }))
            .child(
                Slider::new(state).size_full().child(
                    SliderTrack::new(state)
                        .size_full()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .child(
                            SliderIndicator::new(state)
                                .w_full()
                                .h(px(3.))
                                .rounded(px(2.))
                                .bg(theme.muted)
                                .relative()
                                .child(
                                    div()
                                        .absolute()
                                        .h_full()
                                        .left(relative(filled_from))
                                        .w(relative((share - filled_from).max(0.)))
                                        .rounded(px(2.))
                                        .bg(theme.primary),
                                ),
                        ),
                ),
            )
            .when(clip == Clip::Studio, |bar| range(this, bar, cx))
            .when_some(seek_bar.hover, |bar, (hovered, offset)| {
                bar.child(
                    div()
                        .absolute()
                        .bottom(px(HEIGHT))
                        .left(offset - px(18.))
                        .px_1()
                        .rounded(px(3.))
                        .bg(popover)
                        .border_1()
                        .border_color(border)
                        .text_xs()
                        .child(clock(time_at(length, hovered))),
                )
            }),
    )
    .child(
        div()
            .flex_shrink_0()
            .text_xs()
            .text_color(muted_text)
            .child(time),
    )
    .into_any_element()
}

/// The range of the Studio edit over the bar, with a handle at each end.
fn range(this: &FileBuddy, bar: Stateful<Div>, cx: &mut Context<FileBuddy>) -> Stateful<Div> {
    let Some(edit) = &this.studio.edit else {
        return bar;
    };
    let length = edit.duration_ms.max(1) as f32;
    let start = edit.range.start_ms as f32 / length;
    let end = edit.range.end_ms as f32 / length;
    let color = gpui::Hsla::from(rgb(RANGE_COLOR));
    let handle = |edge: Edge, share: f32| {
        div()
            .id(match edge {
                Edge::Start => "range-start",
                Edge::End => "range-end",
            })
            .absolute()
            .top_0()
            .h_full()
            .left(relative(share))
            .ml(px(-HANDLE_WIDTH / 2.))
            .w(px(HANDLE_WIDTH))
            .flex()
            .justify_center()
            .cursor_ew_resize()
            .child(div().w(px(3.)).h_full().rounded(px(1.)).bg(color))
            // The track under it would seek.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_drag(HandleDrag(edge), |drag, _, _, cx| cx.new(|_| *drag))
    };
    let band = div()
        .absolute()
        .top(px((HEIGHT - 7.) / 2.))
        .h(px(7.))
        .left(relative(start))
        .w(relative(end - start))
        .rounded(px(2.))
        .bg(color.opacity(0.3));
    let (start_handle, end_handle) = (handle(Edge::Start, start), handle(Edge::End, end));
    bar.on_drag_move(
        cx.listener(|this, event: &DragMoveEvent<HandleDrag>, _, cx| {
            let edge = event.drag(cx).0;
            let bounds = this.studio.seek_bar.state.read(cx).bounds();
            if bounds.size.width > px(0.) {
                let share = (event.event.position.x - bounds.left()) / bounds.size.width;
                this.drag_handle(edge, share, cx);
            }
        }),
    )
    .on_drag_move(
        cx.listener(|this, event: &DragMoveEvent<TextEdgeDrag>, _, cx| {
            let TextEdgeDrag(index, edge) = *event.drag(cx);
            let bounds = this.studio.seek_bar.state.read(cx).bounds();
            if bounds.size.width > px(0.) {
                let share = (event.event.position.x - bounds.left()) / bounds.size.width;
                let snap = SNAP / f32::from(bounds.size.width);
                this.drag_text_edge(index, edge, share, snap, cx);
            }
        }),
    )
    .on_drag_move(
        cx.listener(|this, event: &DragMoveEvent<TextBarDrag>, _, cx| {
            let width = this.studio.seek_bar.state.read(cx).bounds().size.width;
            if width > px(0.) {
                this.drag_text_bar(event.event.position.x, width, cx);
            }
        }),
    )
    .on_mouse_up(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| this.release_handle(cx)),
    )
    .on_mouse_up_out(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| this.release_handle(cx)),
    )
    .when(!edit.framed(), |bar| {
        bar.child(band).child(start_handle).child(end_handle)
    })
    .child(text_rows(this, edit, cx))
}

/// One row for each text under the bar, in the room the page leaves for them,
/// which scrolls past five rows. Each bar holds its words. A press on a bar selects its
/// text, a drag on its middle moves it, and a drag on an end changes it.
fn text_rows(
    this: &FileBuddy,
    edit: &crate::studio::Edit,
    cx: &mut Context<FileBuddy>,
) -> gpui::AnyElement {
    let length = edit.duration_ms.max(1) as f32;
    let accent = gpui::Hsla::from(rgb(RANGE_COLOR));
    let theme = cx.theme();
    let (foreground, muted) = (theme.foreground, theme.muted_foreground);
    let rows = edit.texts.iter().enumerate().map(|(index, text)| {
        // In frame mode a delete can leave a text past the end of the strip.
        let start = (text.start_ms as f32 / length).min(1.);
        let end = (text.end_ms as f32 / length).min(1.);
        let selected = this.studio.selected_text == Some(index);
        let top = index as f32 * (TEXT_ROW_HEIGHT + TEXT_ROW_GAP);
        let (fill, ends) = if selected {
            (accent.opacity(0.3), accent)
        } else {
            (foreground.opacity(0.08), muted.opacity(0.7))
        };
        let span = (text.start_ms, text.end_ms);
        let end_grip = |edge: Edge| {
            div()
                .id((
                    match edge {
                        Edge::Start => "text-start",
                        Edge::End => "text-end",
                    },
                    index,
                ))
                .absolute()
                .top_0()
                .h_full()
                .w(px(TEXT_END_WIDTH))
                .map(|grip| match edge {
                    Edge::Start => grip.left_0().rounded_l(px(3.)),
                    Edge::End => grip.right_0().rounded_r(px(3.)),
                })
                .bg(ends)
                .cursor_ew_resize()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.select_text(Some(index), window, cx);
                    }),
                )
                .on_drag(TextEdgeDrag(index, edge), |drag, _, _, cx| {
                    cx.new(|_| *drag)
                })
        };
        let words = text.trimmed().replace('\n', " / ");
        div()
            .id(("text-bar", index))
            .absolute()
            .top(px(top))
            .h(px(TEXT_ROW_HEIGHT))
            .left(relative(start))
            .w(relative((end - start).max(0.)))
            .min_w(px(TEXT_END_WIDTH * 2.))
            .rounded(px(3.))
            .bg(fill)
            .overflow_hidden()
            .px(px(TEXT_END_WIDTH + 3.))
            .flex()
            .items_center()
            .text_xs()
            .text_color(if selected { foreground } else { muted })
            .whitespace_nowrap()
            .cursor_grab()
            .child(div().truncate().child(words))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.studio.text_move = Some((index, event.position.x, span));
                    this.select_text(Some(index), window, cx);
                }),
            )
            .on_drag(TextBarDrag, |drag, _, _, cx| cx.new(|_| *drag))
            .child(end_grip(Edge::Start))
            .child(end_grip(Edge::End))
            .into_any_element()
    });
    div()
        .id("text-rows")
        .absolute()
        .top(px(HEIGHT + TEXT_ROW_GAP))
        .left_0()
        .right_0()
        .h(px(TEXT_ROWS_HEIGHT - TEXT_ROW_GAP))
        .overflow_y_scroll()
        .child(
            div()
                .relative()
                .w_full()
                .h(px(
                    edit.texts.len() as f32 * (TEXT_ROW_HEIGHT + TEXT_ROW_GAP)
                ))
                .children(rows),
        )
        .into_any_element()
}

fn clock(time: Duration) -> String {
    media::clock(time.as_millis() as i64)
}
