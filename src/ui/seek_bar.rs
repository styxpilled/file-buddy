//! The progress bar under the clip in the side pane, with the time beside it.
//! A click seeks, a drag scrubs, and the pointer over the bar shows the time
//! under it. `shift-left` and `shift-right` skip, and a time in the Speech tab
//! seeks.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, MouseMoveEvent, Pixels, Subscription, Window, div, px, relative,
};
use gpui_kit::base::slider::{SliderEvent, SliderState};
use gpui_kit::base::{Slider, SliderIndicator, SliderTrack};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_video::Video;

use crate::media;
use crate::ui::FileBuddy;

/// How often a drag seeks. Each seek restarts the decoder at a keyframe.
const SCRUB_INTERVAL: Duration = Duration::from_millis(60);
/// How far `shift-left` and `shift-right` move.
const SKIP: Duration = Duration::from_secs(5);
pub(super) const HEIGHT: f32 = 16.;

pub(super) struct SeekBar {
    /// The share of the clip played, from 0 to 1.
    state: Entity<SliderState>,
    /// Where a drag holds the bar. The clip catches up at its own pace.
    dragging: Option<f32>,
    /// The pointer over the bar: its share of the clip, and its distance from
    /// the start of the bar.
    hover: Option<(f32, Pixels)>,
    last_scrub: Instant,
    _events: Subscription,
}

impl SeekBar {
    pub(super) fn new(cx: &mut Context<FileBuddy>) -> Self {
        let state = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.0001));
        let events = cx.subscribe(&state, |this, _, event: &SliderEvent, cx| {
            this.scrub(event, cx);
        });
        Self {
            state,
            dragging: None,
            hover: None,
            last_scrub: Instant::now(),
            _events: events,
        }
    }
}

fn time_at(video: &Video, share: f32) -> Duration {
    video.duration().mul_f32(share.clamp(0., 1.))
}

impl FileBuddy {
    /// A drag seeks to keyframes, which is fast. The click, or the end of the
    /// drag, seeks to the exact time.
    fn scrub(&mut self, event: &SliderEvent, cx: &mut Context<Self>) {
        let Some(video) = self.player.video() else {
            return;
        };
        match event {
            SliderEvent::Change(value) => {
                self.seek_bar.dragging = Some(value.end());
                if self.seek_bar.last_scrub.elapsed() >= SCRUB_INTERVAL {
                    self.seek_bar.last_scrub = Instant::now();
                    video.seek(time_at(video, value.end()), false).ok();
                }
            }
            SliderEvent::Release(value) => {
                self.seek_bar.dragging = None;
                video.seek(time_at(video, value.end()), true).ok();
            }
        }
        cx.notify();
    }

    /// `shift-left` and `shift-right`. A skip stops at either end of the clip.
    pub(super) fn skip(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(video) = self.player.video() else {
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
    pub(super) fn sync_seek_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.seek_bar.dragging.is_some() {
            return;
        }
        let Some(video) = self.player.video() else {
            return;
        };
        let length = video.duration().as_secs_f32();
        if length <= 0. {
            return;
        }
        let share = (video.position().as_secs_f32() / length).clamp(0., 1.);
        let state = self.seek_bar.state.clone();
        if (state.read(cx).value().end() - share).abs() > 1e-4 {
            state.update(cx, |state, cx| state.set_value(share, window, cx));
        }
    }
}

/// The bar and the time, or the empty track alone when the pane has no clip, so
/// the stage above it does not move and a still reads as a clip with no time. It never shrinks: the empty
/// row has no content to hold it, and a long panel below would squeeze it.
pub(super) fn bar(this: &FileBuddy, width: f32, cx: &mut Context<FileBuddy>) -> AnyElement {
    let row = h_flex()
        .w(px(width))
        .h(px(HEIGHT))
        .flex_shrink_0()
        .gap_2()
        .items_center();
    let Some(video) = this.player.video() else {
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
    let seek_bar = &this.seek_bar;
    let state = &seek_bar.state;
    let share = seek_bar
        .dragging
        .unwrap_or_else(|| state.read(cx).value().end());
    let theme = cx.theme();
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
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let bounds = this.seek_bar.state.read(cx).bounds();
                if bounds.size.width <= px(0.) {
                    return;
                }
                let offset = (event.position.x - bounds.left()).clamp(px(0.), bounds.size.width);
                this.seek_bar.hover = Some((offset / bounds.size.width, offset));
                cx.notify();
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered {
                    this.seek_bar.hover = None;
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
            .when_some(seek_bar.hover, |bar, (hovered, offset)| {
                bar.child(
                    div()
                        .absolute()
                        .bottom(px(HEIGHT))
                        .left(offset - px(18.))
                        .px_1()
                        .rounded(px(3.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .text_xs()
                        .child(clock(time_at(video, hovered))),
                )
            }),
    )
    .child(
        div()
            .flex_shrink_0()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(time),
    )
    .into_any_element()
}

fn clock(time: Duration) -> String {
    media::clock(time.as_millis() as i64)
}
