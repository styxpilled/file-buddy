//! Middle-click autoscroll on the grid and the table, as in a browser on
//! Windows. A click on the wheel sets an anchor, and the list scrolls while the
//! pointer is away from it. A drag with the wheel held does the same until the
//! release. Any click, a wheel turn or escape ends it.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    Context, CursorStyle, DispatchPhase, Div, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollWheelEvent, Window, canvas, div, point, px,
};

use windows::Win32::UI::WindowsAndMessaging::{IDC_SIZENS, LoadCursorW, SetCursor};

use crate::ui::FileBuddy;

/// No scroll this near the anchor, so a hand that does not stay still does
/// not move the list.
const DEAD_ZONE: f32 = 12.;
/// Pixels a second for each pixel past the dead zone.
const SPEED: f32 = 10.;
/// A frame that comes late, after a stall, scrolls no more than this.
const LONGEST_STEP: Duration = Duration::from_millis(50);

pub(super) struct Autoscroll {
    anchor: Point<Pixels>,
    pointer: Point<Pixels>,
    /// The wheel is still down from the press that started it.
    held: bool,
    /// The pointer left the dead zone with the wheel down, so the release
    /// ends it.
    dragged: bool,
    last_step: Instant,
    /// The tile under the pointer. Its clip waits until the list stops.
    hovered: Option<usize>,
}

/// Pixels a second, down when positive.
fn speed(anchor: Pixels, pointer: Pixels) -> f32 {
    let distance = f32::from(pointer - anchor);
    let past = distance.abs() - DEAD_ZONE;
    if past <= 0. {
        0.
    } else {
        past * SPEED * distance.signum()
    }
}

impl FileBuddy {
    fn start_autoscroll(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        // The window holds the mouse while the wheel is down, and Windows asks
        // it for no cursor until the release, so the cursor is set here.
        // SAFETY: a system cursor is shared and never freed.
        unsafe {
            if let Ok(cursor) = LoadCursorW(None, IDC_SIZENS) {
                SetCursor(Some(cursor));
            }
        }
        self.autoscroll = Some(Autoscroll {
            anchor: position,
            pointer: position,
            held: true,
            dragged: false,
            last_step: Instant::now(),
            hovered: None,
        });
        cx.notify();
    }

    pub(super) fn end_autoscroll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(autoscroll) = self.autoscroll.take() else {
            return false;
        };
        if let Some(index) = autoscroll.hovered {
            self.set_hovered(index, true, cx);
        }
        cx.notify();
        true
    }

    /// Hover while the list moves under the pointer. Returns false when the
    /// tile must not play yet.
    pub(super) fn autoscroll_hover(&mut self, index: usize, hovered: bool) -> bool {
        let Some(autoscroll) = &mut self.autoscroll else {
            return true;
        };
        if hovered {
            autoscroll.hovered = Some(index);
        } else if autoscroll.hovered == Some(index) {
            autoscroll.hovered = None;
        }
        !hovered
    }

    /// Move the list for the time since the last frame, before the list lays
    /// itself out.
    pub(super) fn step_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.autoscroll.is_some()
            && (!window.is_window_active() || self.results.is_empty() || self.settings().is_some())
        {
            self.end_autoscroll(cx);
        }
        let Some(autoscroll) = &mut self.autoscroll else {
            return;
        };
        let now = Instant::now();
        let elapsed = (now - autoscroll.last_step).min(LONGEST_STEP);
        autoscroll.last_step = now;
        let speed = speed(autoscroll.anchor.y, autoscroll.pointer.y);
        if speed == 0. {
            return;
        }
        let offset = self.scroll.offset();
        let lowest = -self.scroll.max_offset().y;
        let y = (offset.y - px(speed * elapsed.as_secs_f32())).clamp(lowest, px(0.));
        self.scroll.set_offset(point(offset.x, y));
        window.request_animation_frame();
    }

    fn autoscroll_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(autoscroll) = &mut self.autoscroll else {
            return;
        };
        autoscroll.pointer = position;
        if autoscroll.held && (position - autoscroll.anchor).magnitude() > DEAD_ZONE as f64 {
            autoscroll.dragged = true;
        }
        cx.notify();
    }

    fn autoscroll_released(&mut self, cx: &mut Context<Self>) {
        let Some(autoscroll) = &mut self.autoscroll else {
            return;
        };
        autoscroll.held = false;
        if autoscroll.dragged {
            self.end_autoscroll(cx);
        }
    }
}

/// The box that holds the list, with autoscroll on it.
pub(super) fn attach(this: &FileBuddy, list: impl IntoElement, cx: &mut Context<FileBuddy>) -> Div {
    div()
        .relative()
        .size_full()
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.start_autoscroll(event.position, cx)
            }),
        )
        .child(list)
        .when(this.autoscroll.is_some(), |container| {
            container.child(listeners(cx))
        })
}

/// The listeners of the window while autoscroll is on. They take events the
/// pointer sends anywhere, not only over the list.
fn listeners(cx: &mut Context<FileBuddy>) -> impl IntoElement {
    let view = cx.entity();
    canvas(
        |_, _, _| (),
        move |_, (), window, _| {
            window.set_window_cursor_style(CursorStyle::ResizeUpDown);
            window.on_mouse_event({
                let view = view.clone();
                move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        view.update(cx, |this, cx| this.autoscroll_moved(event.position, cx));
                    }
                }
            });
            window.on_mouse_event({
                let view = view.clone();
                move |event: &MouseDownEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let held = view.read(cx).autoscroll.as_ref().is_some_and(|a| a.held);
                    if !held || event.button != MouseButton::Middle {
                        view.update(cx, |this, cx| this.end_autoscroll(cx));
                        cx.stop_propagation();
                    }
                }
            });
            window.on_mouse_event({
                let view = view.clone();
                move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Middle {
                        view.update(cx, |this, cx| this.autoscroll_released(cx));
                    }
                }
            });
            window.on_mouse_event(move |_: &ScrollWheelEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    view.update(cx, |this, cx| this.end_autoscroll(cx));
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speed_grows_past_the_dead_zone() {
        assert_eq!(speed(px(100.), px(100.)), 0.);
        assert_eq!(speed(px(100.), px(100. + DEAD_ZONE)), 0.);
        assert_eq!(speed(px(100.), px(122.)), 100.);
        assert_eq!(speed(px(100.), px(78.)), -100.);
        assert!(speed(px(100.), px(200.)) > speed(px(100.), px(150.)));
    }
}
