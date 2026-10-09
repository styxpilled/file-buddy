//! The progress bar under the clip in the side pane and in the Studio, with the
//! time beside it. A click seeks, a drag scrubs, and the pointer over the bar
//! shows the time under it. `shift-left` and `shift-right` skip, and a time in
//! the Speech tab seeks. In the Studio the bar also holds the range, with a
//! handle at each end.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, DragMoveEvent, Entity, MouseButton, MouseMoveEvent, Pixels, Render,
    Stateful, Subscription, Window, div, px, relative,
};
use gpui_kit::base::slider::{SliderEvent, SliderState};
use gpui_kit::base::{Slider, SliderIndicator, SliderTrack};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_video::Video;

use crate::media;
use crate::studio::Edge;
use crate::ui::{FileBuddy, Page};

/// How often a drag seeks. Each seek restarts the decoder at a keyframe.
const SCRUB_INTERVAL: Duration = Duration::from_millis(60);
/// How far `shift-left` and `shift-right` move.
const SKIP: Duration = Duration::from_secs(5);
pub(super) const HEIGHT: f32 = 16.;
const HANDLE_WIDTH: f32 = 10.;

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
    last_scrub: Instant,
    /// Whether the clip was paused before a drag paused it. A clip that plays
    /// on while the bar moves fights every seek.
    paused_before: Option<bool>,
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
            last_scrub: Instant::now(),
            paused_before: None,
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

fn time_at(video: &Video, share: f32) -> Duration {
    video.duration().mul_f32(share.clamp(0., 1.))
}

impl SeekBar {
    fn hold(&mut self, video: &Video) {
        if self.paused_before.is_none() {
            self.paused_before = Some(video.paused());
            video.set_paused(true);
        }
    }

    /// After the exact seek, so a clip at its end does not restart at zero.
    fn let_go(&mut self, video: &Video) {
        if let Some(paused) = self.paused_before.take() {
            video.set_paused(paused);
        }
    }
}

impl FileBuddy {
    fn clip_video(&self, clip: Clip) -> Option<&Video> {
        match clip {
            Clip::Pane => self.player.video(),
            Clip::Studio => self.studio.video.as_ref(),
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

    /// A drag seeks to keyframes, which is fast. The click, or the end of the
    /// drag, seeks to the exact time.
    fn scrub(&mut self, clip: Clip, event: &SliderEvent, cx: &mut Context<Self>) {
        let Some(video) = self.clip_video(clip).cloned() else {
            return;
        };
        let bar = self.bar_of(clip);
        match event {
            SliderEvent::Change(value) => {
                bar.dragging = Some(value.end());
                bar.hold(&video);
                if bar.last_scrub.elapsed() >= SCRUB_INTERVAL {
                    bar.last_scrub = Instant::now();
                    video.seek(time_at(&video, value.end()), false).ok();
                }
            }
            SliderEvent::Release(value) => {
                bar.dragging = None;
                video.seek(time_at(&video, value.end()), true).ok();
                bar.let_go(&video);
            }
        }
        cx.notify();
    }

    /// A drag on a handle moves it, and the clip follows it to keyframes. The
    /// release seeks to the exact time.
    fn drag_handle(&mut self, edge: Edge, share: f32, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.studio.edit else {
            return;
        };
        let at = (edit.duration_ms as f32 * share.clamp(0., 1.)) as u64;
        edit.drag(edge, at);
        let at = edit.range.at(edge);
        self.studio.handle = Some(edge);
        let bar = &mut self.studio.seek_bar;
        if let Some(video) = &self.studio.video {
            bar.hold(video);
            if bar.last_scrub.elapsed() >= SCRUB_INTERVAL {
                bar.last_scrub = Instant::now();
                video.seek(Duration::from_millis(at), false).ok();
            }
        }
        cx.notify();
    }

    fn release_handle(&mut self, cx: &mut Context<Self>) {
        let Some(edge) = self.studio.handle.take() else {
            return;
        };
        if let (Some(edit), Some(video)) = (&self.studio.edit, &self.studio.video) {
            video
                .seek(Duration::from_millis(edit.range.at(edge)), true)
                .ok();
            self.studio.seek_bar.let_go(video);
        }
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
    pub(super) fn sync_seek_bar(
        &mut self,
        clip: Clip,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.bar_of(clip).dragging.is_some() {
            return;
        }
        let Some(video) = self.clip_video(clip) else {
            return;
        };
        let length = video.duration().as_secs_f32();
        if length <= 0. {
            return;
        }
        let share = (video.position().as_secs_f32() / length).clamp(0., 1.);
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
    let Some(video) = this.clip_video(clip) else {
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
    let theme = cx.theme();
    let (popover, border, muted_text) = (theme.popover, theme.border, theme.muted_foreground);
    let time = format!(
        "{} / {}",
        clock(time_at(video, share)),
        clock(video.duration())
    );

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
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(share))
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
                        .child(clock(time_at(video, hovered))),
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
    let theme = cx.theme();
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
            .child(div().w(px(3.)).h_full().rounded(px(1.)).bg(theme.warning))
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
        .bg(theme.warning.opacity(0.3));
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
    .on_mouse_up(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| this.release_handle(cx)),
    )
    .on_mouse_up_out(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| this.release_handle(cx)),
    )
    .child(band)
    .child(start_handle)
    .child(end_handle)
}

fn clock(time: Duration) -> String {
    media::clock(time.as_millis() as i64)
}
