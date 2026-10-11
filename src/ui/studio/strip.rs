//! Frame mode in the Studio: the strip of frames over the timeline, each with
//! its delay, the frame the stage shows, and playback by the delays. A click
//! selects a frame, shift-click a run and ctrl-click one more, and a drag
//! moves the selected frames. The frames decode on a background thread and
//! join the strip as they come, so the edit opens on the first one.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, DragMoveEvent, Entity, Hsla, Modifiers, MouseButton,
    MouseDownEvent, Render, RenderImage, ScrollHandle, Subscription, Task, UniformListScrollHandle,
    Window, div, img, px, rgb,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use image::RgbaImage;

use super::frame_in;
use crate::actions::clipboard;
use crate::ffmpeg::export;
use crate::studio::Edit;
use crate::studio::frames::{self, Frame, MIN_DELAY_MS};
use crate::ui::seek_bar::{self, Clip};
use crate::ui::{FileBuddy, toggle};

const CELL_WIDTH: f32 = 64.;
const CELL_GAP: f32 = 4.;
const THUMB_HEIGHT: f32 = 40.;
/// The small copies are this many pixels high, so they stay sharp in the grid
/// at 200%.
const THUMB_PIXELS: u32 = 192;
const DELAY_LINE: f32 = 16.;
/// The room for the scrollbar under the cells.
const SCROLLBAR: f32 = 10.;
/// The height of the strip on the page.
pub(super) const HEIGHT: f32 = THUMB_HEIGHT + DELAY_LINE + SCROLLBAR + 4.;
/// The input of the delay.
const DELAY_INPUT: f32 = 64.;
/// A skipped frame, on the strip and in the grid.
pub(super) const SKIPPED_OPACITY: f32 = 0.35;

pub(in crate::ui) struct Strip {
    pub(super) selected: BTreeSet<usize>,
    /// Where the last plain click was. Shift-click selects from here.
    anchor: usize,
    /// The playhead, in milliseconds of the strip.
    at_ms: f64,
    /// When the playhead last moved, while the frames play.
    clock: Option<Instant>,
    /// Draws the window again when the next frame is due.
    tick: Option<Task<()>>,
    /// The frame a click put on the stage. A skipped frame has no time, so
    /// the playhead alone cannot show it.
    held: Option<usize>,
    /// The small copies of the strip, by image. A copy of a frame shares one.
    pub(super) thumbs: HashMap<usize, Arc<RenderImage>>,
    /// The frame on the stage, at full size, by image.
    shown: Option<(usize, Arc<RenderImage>)>,
    /// The gap the drag would put the selected frames in.
    pub(super) drop_gap: Option<usize>,
    /// A drag just moved the frames, so its release is no click.
    moved: bool,
    scroll: ScrollHandle,
    /// Every frame at once, beside or above the stage.
    pub(super) grid: bool,
    pub(super) grid_scroll: UniformListScrollHandle,
    pub(in crate::ui) delay_input: Entity<InputState>,
    _delay_events: Subscription,
}

/// The selected frames, while a drag holds them.
#[derive(Clone, Copy)]
pub(super) struct FrameDrag;

impl Render for FrameDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

pub(super) fn key(frame: &Frame) -> usize {
    Arc::as_ptr(&frame.image) as usize
}

/// GPUI draws BGRA.
fn render_image(rgba: &RgbaImage) -> Arc<RenderImage> {
    let mut bgra = rgba.clone();
    for pixel in bgra.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(bgra)]))
}

fn thumbnail(frame: &Frame) -> Arc<RenderImage> {
    let (width, height) = frame.image.dimensions();
    let width = (width * THUMB_PIXELS / height.max(1)).max(1);
    render_image(&image::imageops::thumbnail(
        frame.image.as_ref(),
        width,
        THUMB_PIXELS,
    ))
}

/// The size of a frame of `size` that fits in `room`, whole.
pub(super) fn fitted(size: (u32, u32), (width, height): (f32, f32)) -> (f32, f32) {
    if size.0 == 0 || size.1 == 0 {
        return (width, height);
    }
    let scale = (width / size.0 as f32).min(height / size.1 as f32);
    (size.0 as f32 * scale, size.1 as f32 * scale)
}

impl Strip {
    pub(super) fn new(window: &mut Window, cx: &mut Context<FileBuddy>) -> Self {
        let delay_input = cx.new(|cx| InputState::new(window, cx).placeholder("ms"));
        let delay_events = cx.subscribe_in(
            &delay_input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                let typed = input.read(cx).value().trim().parse::<u32>().ok();
                match event {
                    // A delay under the floor may still be typed on the way to
                    // a longer one.
                    InputEvent::Change => {
                        if let Some(delay @ MIN_DELAY_MS..) = typed {
                            this.set_frame_delay(delay, cx);
                        }
                    }
                    InputEvent::PressEnter { .. } => {
                        if let Some(delay) = typed {
                            this.set_frame_delay(delay, cx);
                        }
                        this.sync_delay_input(window, cx);
                        window.focus(&this.studio.focus, cx);
                    }
                    _ => {}
                }
            },
        );
        Self {
            selected: BTreeSet::new(),
            anchor: 0,
            at_ms: 0.,
            clock: None,
            tick: None,
            held: None,
            thumbs: HashMap::new(),
            shown: None,
            drop_gap: None,
            moved: false,
            scroll: ScrollHandle::new(),
            grid: false,
            grid_scroll: UniformListScrollHandle::new(),
            delay_input,
            _delay_events: delay_events,
        }
    }

    pub(in crate::ui) fn at_ms(&self) -> u64 {
        self.at_ms as u64
    }

    /// Scrolls the strip so the cell shows. The cell may not be built, so this
    /// goes by its place and not by its element.
    fn reveal(&mut self, index: usize, len: usize) {
        let width = f32::from(self.scroll.bounds().size.width);
        let step = CELL_WIDTH + CELL_GAP;
        let left = -f32::from(self.scroll.offset().x);
        let at = index as f32 * step;
        let to = if at < left {
            at
        } else if at + CELL_WIDTH > left + width {
            at + CELL_WIDTH - width
        } else {
            return;
        };
        let most = (len as f32 * step - CELL_GAP - width).max(0.);
        self.scroll
            .set_offset(gpui::point(px(-to.clamp(0., most)), px(0.)));
    }

    /// The page closed. The clock starts again when the frames play.
    pub(super) fn stop(&mut self) {
        self.clock = None;
        self.tick = None;
        self.drop_gap = None;
    }
}

impl FileBuddy {
    /// A new source. The images of the last one leave the atlas.
    pub(super) fn reset_strip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strip = &mut self.studio.strip;
        let old: Vec<_> = std::mem::take(&mut strip.thumbs)
            .into_values()
            .chain(strip.shown.take().map(|(_, image)| image))
            .collect();
        for image in old {
            cx.drop_image(image, Some(window));
        }
        let strip = &mut self.studio.strip;
        strip.selected.clear();
        strip.anchor = 0;
        strip.at_ms = 0.;
        strip.held = None;
        strip.stop();
        strip.moved = false;
        self.sync_delay_input(window, cx);
    }

    /// Opens a GIF or an animated WebP in frame mode. The frames decode away
    /// from the window, and the edit opens with the first of them.
    pub(super) fn open_frames(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flash("opening the frames", false, cx);
        let (sender, mut decoded) = mpsc::unbounded();
        let source = path.clone();
        let decoding = cx.background_executor().spawn(async move {
            frames::decode_each(&source, |frame| {
                let thumb = thumbnail(&frame);
                sender.unbounded_send((frame, thumb)).is_ok()
            })
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let mut opened = false;
            while let Some(next) = decoded.next().await {
                let mut batch = vec![next];
                while let Ok(more) = decoded.try_recv() {
                    batch.push(more);
                }
                let path = path.clone();
                let added = this.update_in(cx, |this, window, cx| {
                    this.add_frames(path, batch, opened, window, cx)
                });
                if added.is_err() {
                    return;
                }
                opened = true;
            }
            let result = decoding.await;
            this.update(cx, |this, cx| {
                this.studio.opening = None;
                if let Err(e) = result {
                    log::warn!("cannot decode {}: {e:#}", path.display());
                    let message = if opened {
                        "some frames cannot open"
                    } else {
                        "this file cannot open"
                    };
                    this.flash(message, true, cx);
                }
                cx.notify();
            })
            .ok();
        });
        self.studio.opening = Some(task);
    }

    /// Frames that just decoded. The first of them open the edit.
    fn add_frames(
        &mut self,
        path: std::path::PathBuf,
        batch: Vec<(Frame, Arc<RenderImage>)>,
        opened: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (frames, thumbs): (Vec<_>, Vec<_>) = batch.into_iter().unzip();
        let keys: Vec<_> = frames.iter().map(key).collect();
        if opened {
            if let Some(edit) = &mut self.studio.edit {
                edit.push_frames(frames);
            }
        } else {
            self.notice = None;
            self.start_edit(Edit::with_frames(path, frames), None, window, cx);
            self.reset_strip(window, cx);
        }
        self.studio
            .strip
            .thumbs
            .extend(keys.into_iter().zip(thumbs));
        cx.notify();
    }

    pub(in crate::ui) fn framed(&self) -> bool {
        self.studio.edit.as_ref().is_some_and(Edit::framed)
    }

    /// The frames of the edit still decode.
    pub(super) fn frames_loading(&self) -> bool {
        self.framed() && self.studio.opening.is_some()
    }

    /// Where the Studio is, in milliseconds of the source or of the strip.
    pub(in crate::ui) fn studio_at_ms(&self) -> u64 {
        self.clip_times(Clip::Studio)
            .map_or(0, |(at, _)| at.as_millis() as u64)
    }

    /// A time in the Studio, from the bar or a text.
    pub(in crate::ui) fn seek_studio(&mut self, to: Duration) {
        match &self.studio.video {
            Some(video) => {
                video.seek(to, true).ok();
            }
            None => self.seek_strip(to),
        }
    }

    pub(in crate::ui) fn seek_strip(&mut self, to: Duration) {
        let length = self.studio.edit.as_ref().map_or(0, |edit| edit.duration_ms);
        let strip = &mut self.studio.strip;
        strip.at_ms = (to.as_millis() as u64).min(length.saturating_sub(1)) as f64;
        strip.clock = None;
        strip.held = None;
    }

    /// Called on every frame of the page. The playhead runs over the range at
    /// the speed of the edit and the speed button, and the stage holds the
    /// frame under it. The window draws again when the next frame is due, not
    /// at the rate of the display.
    pub(super) fn step_strip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.studio.edit.as_ref().filter(|edit| edit.framed()) else {
            return;
        };
        let (start, end) = (edit.range.start_ms as f64, edit.range.end_ms as f64);
        let playing = self.studio.transport.playing()
            && self.studio.seek_bar.dragging.is_none()
            && end > start;
        let speed = self.clip_speed(Clip::Studio);
        let strip = &mut self.studio.strip;
        let index = if playing {
            strip.held = None;
            let now = Instant::now();
            if let Some(then) = strip.clock {
                strip.at_ms += (now - then).as_secs_f64() * 1000. * speed;
            }
            if strip.at_ms >= end {
                strip.at_ms = start + (strip.at_ms - end) % (end - start);
            }
            strip.clock = Some(now);
            let index = edit.frame_at(strip.at_ms as u64);
            let next = (edit.frame_start_ms(index + 1) as f64).min(end);
            let wait = Duration::from_secs_f64((next - strip.at_ms).max(1.) / speed / 1000.);
            strip.tick = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            }));
            index
        } else {
            strip.clock = None;
            strip.tick = None;
            strip
                .held
                .filter(|&held| held < edit.frames.len())
                .unwrap_or_else(|| edit.frame_at(strip.at_ms as u64))
        };

        let frame = &edit.frames[index];
        if strip
            .shown
            .as_ref()
            .is_some_and(|(shown, _)| *shown == key(frame))
        {
            return;
        }
        let image = render_image(&frame.image);
        if let Some((_, old)) = strip.shown.replace((key(frame), image)) {
            cx.drop_image(old, Some(window));
        }
        if playing {
            strip.reveal(index, edit.frames.len());
        }
    }

    /// A press on a frame. Shift and ctrl change the selection at once. A
    /// press on a frame already selected waits for the release, so a drag can
    /// move the whole selection.
    pub(super) fn press_frame(
        &mut self,
        index: usize,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strip = &mut self.studio.strip;
        strip.moved = false;
        let plain = !modifiers.shift && !modifiers.control;
        if plain && strip.selected.contains(&index) {
            return;
        }
        frames::click(
            &mut strip.selected,
            &mut strip.anchor,
            index,
            modifiers.shift,
            modifiers.control,
        );
        self.show_frame(index, window, cx);
    }

    /// The release of a plain press on a selected frame, with no drag.
    pub(super) fn click_frame(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.modifiers();
        let strip = &mut self.studio.strip;
        if std::mem::take(&mut strip.moved) || modifiers.shift || modifiers.control {
            return;
        }
        frames::click(&mut strip.selected, &mut strip.anchor, index, false, false);
        self.show_frame(index, window, cx);
    }

    /// The stage shows the frame and the frames stop. A frame and a text are
    /// never selected together, so `delete` has one target.
    fn show_frame(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(edit) = &self.studio.edit {
            self.studio.strip.at_ms = edit.frame_start_ms(index) as f64;
            self.studio.strip.held = Some(index);
        }
        self.studio.transport.paused = true;
        self.select_text(None, window, cx);
        window.focus(&self.studio.focus, cx);
        self.sync_delay_input(window, cx);
        cx.notify();
    }

    /// The selection changes, so the delay shows what the selected frames
    /// share.
    pub(super) fn sync_delay_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let delay = self
            .studio
            .edit
            .as_ref()
            .and_then(|edit| edit.shared_delay(&self.studio.strip.selected))
            .map(|delay| delay.to_string())
            .unwrap_or_default();
        self.studio
            .strip
            .delay_input
            .update(cx, |input, cx| input.set_value(delay, window, cx));
    }

    fn set_frame_delay(&mut self, delay: u32, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.studio.edit {
            edit.set_delay(&self.studio.strip.selected, delay);
            cx.notify();
        }
    }

    /// `delete`, with frames selected. One frame always stays.
    pub(super) fn delete_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.studio.edit else {
            return;
        };
        let Some(next) = edit.delete_frames(&self.studio.strip.selected) else {
            self.flash("an animation keeps one frame", true, cx);
            return;
        };
        let strip = &mut self.studio.strip;
        strip.selected = BTreeSet::from([next]);
        strip.anchor = next;
        self.show_frame(next, window, cx);
    }

    /// `ctrl-d`.
    pub(super) fn duplicate_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.studio.edit.as_mut().filter(|edit| edit.framed()) else {
            return;
        };
        let copies = edit.duplicate_frames(&self.studio.strip.selected);
        let Some(&first) = copies.first() else {
            return;
        };
        let strip = &mut self.studio.strip;
        strip.selected = copies;
        strip.anchor = first;
        self.show_frame(first, window, cx);
        let len = self
            .studio
            .edit
            .as_ref()
            .map_or(0, |edit| edit.frames.len());
        self.studio.strip.reveal(first, len);
    }

    /// `s` skips the selected frames, or plays them again. The Skip button of a
    /// frame in the grid does it to that frame alone.
    pub(super) fn skip_frames(&mut self, only: Option<usize>, cx: &mut Context<Self>) {
        let Some(edit) = self.studio.edit.as_mut().filter(|edit| edit.framed()) else {
            return;
        };
        let chosen = match only {
            Some(index) => BTreeSet::from([index]),
            None => self.studio.strip.selected.clone(),
        };
        if !edit.toggle_skip(&chosen) && !chosen.is_empty() {
            self.flash("an animation keeps one frame", true, cx);
        }
        cx.notify();
    }

    /// The Copy button of a frame in the grid. The frame goes to the clipboard
    /// as `p` puts the stage there: with the crop and the texts.
    pub(super) fn copy_frame(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(edit) = self.studio.edit.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let copied = cx
                .background_executor()
                .spawn(async move {
                    export::frame_screenshot(&edit, index)
                        .and_then(|png| clipboard::copy_image(&png))
                })
                .await;
            this.update(cx, |this, cx| match copied {
                Ok(_) => this.flash(format!("copied frame {}", index + 1), false, cx),
                Err(e) => {
                    log::error!("copy of frame {index} failed: {e:#}");
                    this.flash(format!("copy failed: {e}"), true, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// `g`.
    pub(super) fn toggle_frame_grid(&mut self, cx: &mut Context<Self>) {
        if self.framed() {
            self.studio.strip.grid = !self.studio.strip.grid;
            cx.notify();
        }
    }

    /// The pointer moves with frames held. The gap is the one nearest it.
    fn drag_frames(&mut self, x: f32, left: f32, cx: &mut Context<Self>) {
        let Some(edit) = &self.studio.edit else {
            return;
        };
        let strip = &mut self.studio.strip;
        let offset = f32::from(strip.scroll.offset().x);
        let along = (x - left - offset) / (CELL_WIDTH + CELL_GAP);
        let gap = (along + 0.5).floor().clamp(0., edit.frames.len() as f32) as usize;
        self.set_drop_gap(gap, cx);
    }

    pub(super) fn set_drop_gap(&mut self, gap: usize, cx: &mut Context<Self>) {
        let strip = &mut self.studio.strip;
        if strip.drop_gap != Some(gap) {
            strip.drop_gap = Some(gap);
            cx.notify();
        }
    }

    pub(super) fn drop_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(gap) = self.studio.strip.drop_gap.take() else {
            return;
        };
        let Some(edit) = &mut self.studio.edit else {
            return;
        };
        let moved = edit.move_frames(&self.studio.strip.selected, gap);
        let Some(&first) = moved.first() else {
            return;
        };
        let strip = &mut self.studio.strip;
        strip.selected = moved;
        strip.anchor = first;
        strip.moved = true;
        self.show_frame(first, window, cx);
    }
}

/// The frame under the playhead, letterboxed as a video is.
pub(super) fn stage_frame(this: &FileBuddy, width: f32, height: f32) -> Option<AnyElement> {
    let edit = this.studio.edit.as_ref().filter(|edit| edit.framed())?;
    let (_, image) = this.studio.strip.shown.as_ref()?;
    let frame = frame_in(edit.size, (width, height))?;
    Some(
        img(image.clone())
            .w(px(edit.size.0 as f32 * frame.scale))
            .h(px(edit.size.1 as f32 * frame.scale))
            .into_any_element(),
    )
}

/// The small copy of a frame, whole inside `room`. GPUI keeps an image at its
/// own aspect, so a box of another aspect would cut it.
pub(super) fn thumb(this: &FileBuddy, edit: &Edit, frame: &Frame, room: (f32, f32)) -> AnyElement {
    let (width, height) = fitted(edit.size, room);
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .children(
            this.studio
                .strip
                .thumbs
                .get(&key(frame))
                .map(|thumb| img(thumb.clone()).w(px(width)).h(px(height))),
        )
        .into_any_element()
}

/// The cells in sight, with a few on each side.
fn cells_in_sight(scroll: &ScrollHandle, len: usize, width: f32) -> std::ops::Range<usize> {
    let step = CELL_WIDTH + CELL_GAP;
    let left = -f32::from(scroll.offset().x);
    let last = (((left + width) / step).ceil().max(0.) as usize + 2).min(len);
    let first = ((left / step).floor().max(0.) as usize).saturating_sub(2);
    first.min(last)..last
}

/// Every frame with its delay, in a row that scrolls. Only the cells in sight
/// are built, because the window draws the strip again on every frame while
/// the frames play. The frame under the playhead has its delay in the accent
/// color, and a skipped frame is faint.
pub(super) fn strip(
    this: &FileBuddy,
    edit: &Edit,
    width: f32,
    cx: &mut Context<FileBuddy>,
) -> AnyElement {
    let strip = &this.studio.strip;
    let theme = cx.theme();
    let accent = Hsla::from(rgb(seek_bar::RANGE_COLOR));
    let (muted, secondary) = (theme.muted_foreground, theme.secondary);
    let current = edit.frame_at(strip.at_ms());
    // The border takes 2 pixels on each side.
    let room = (CELL_WIDTH - 4., THUMB_HEIGHT - 4.);
    let len = edit.frames.len();
    let step = CELL_WIDTH + CELL_GAP;
    let cells = cells_in_sight(&strip.scroll, len, width).map(|index| {
        let frame = &edit.frames[index];
        let selected = strip.selected.contains(&index);
        let gap_before = strip.drop_gap == Some(index);
        div()
            .id(("strip-frame", index))
            .absolute()
            .top_0()
            .left(px(index as f32 * step))
            .w(px(CELL_WIDTH))
            .flex()
            .flex_col()
            .cursor_pointer()
            .child(
                div()
                    .h(px(THUMB_HEIGHT))
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
                    .child(
                        div()
                            .size_full()
                            .when(frame.skipped, |thumb| thumb.opacity(SKIPPED_OPACITY))
                            .child(thumb(this, edit, frame, room)),
                    ),
            )
            .child(
                div()
                    .h(px(DELAY_LINE))
                    .text_xs()
                    .text_center()
                    .text_color(if index == current && !frame.skipped {
                        accent
                    } else {
                        muted
                    })
                    .child(if frame.skipped {
                        "skip".to_string()
                    } else {
                        format!("{} ms", frame.delay_ms)
                    }),
            )
            .when(gap_before, |cell| {
                cell.child(gap_marker(accent, -CELL_GAP / 2. - 1.))
            })
            .when(strip.drop_gap == Some(len) && index + 1 == len, |cell| {
                cell.child(gap_marker(accent, CELL_WIDTH + CELL_GAP / 2. - 1.))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.press_frame(index, event.modifiers, window, cx)
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.click_frame(index, event, window, cx)
            }))
            .on_drag(FrameDrag, |drag, _, _, cx| cx.new(|_| *drag))
    });
    v_flex()
        .relative()
        .w(px(width))
        .h(px(HEIGHT))
        .flex_shrink_0()
        .child(
            div()
                .id("strip")
                .track_scroll(&strip.scroll)
                .overflow_x_scroll()
                .w_full()
                .h_full()
                .flex()
                .on_drag_move(
                    cx.listener(|this, event: &DragMoveEvent<FrameDrag>, _, cx| {
                        let left = f32::from(event.bounds.left());
                        this.drag_frames(f32::from(event.event.position.x), left, cx)
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
                    div()
                        .relative()
                        .flex_shrink_0()
                        .h_full()
                        .w(px((len as f32 * step - CELL_GAP).max(0.)))
                        .children(cells),
                ),
        )
        .horizontal_scrollbar(&strip.scroll)
        .into_any_element()
}

fn gap_marker(color: Hsla, left: f32) -> impl IntoElement {
    marker(color, left, THUMB_HEIGHT)
}

/// Where a drag would put the selected frames, on the strip or in the grid.
pub(super) fn marker(color: Hsla, left: f32, height: f32) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .left(px(left))
        .w(px(2.))
        .h(px(height))
        .bg(color)
}

/// The panel in frame mode: the count and length, the grid toggle, and the
/// delay of the selected frames with the keys that skip, copy and remove them.
pub(super) fn panel(this: &FileBuddy, edit: &Edit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let selected = this.studio.strip.selected.len();
    let loading = this.frames_loading();
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let length = |ms: u64| crate::media::duration(ms as f64 / 1000.);
    let skipped = edit.frames.iter().filter(|frame| frame.skipped).count();
    let (first, last) = edit.frame_range;
    let count = match (loading, skipped) {
        (true, _) => format!("{} frames, loading", edit.frames.len()),
        (false, 0) => format!("{} frames", edit.frames.len()),
        (false, n) => format!("{} frames, {n} skipped", edit.frames.len()),
    };
    let action = |id: &'static str, label: &'static str, key: &'static str| {
        Button::new(id)
            .xsmall()
            .label(label)
            .tooltip(key)
            .disabled(selected == 0)
            .when(selected > 0, |button| button.cursor_pointer())
    };
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(div().flex_1().child(count))
                .child(
                    toggle("frames-grid", "Grid", this.studio.strip.grid)
                        .xsmall()
                        .tooltip("G")
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_frame_grid(cx))),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .text_color(muted)
                .child(
                    div()
                        .flex_1()
                        .child(format!("Frames {} - {}", first + 1, last + 1)),
                )
                .child(
                    div()
                        .text_color(theme.foreground)
                        .child(length(edit.output_ms())),
                )
                .child(format!("/ {}", length(edit.duration_ms))),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(div().text_color(muted).child("Delay"))
                .child(
                    div().w(px(DELAY_INPUT)).child(
                        Input::new(&this.studio.strip.delay_input)
                            .xsmall()
                            .disabled(selected == 0),
                    ),
                )
                .child(div().text_color(muted).child("ms"))
                .child(div().flex_1())
                .child(
                    action("frames-skip", "Skip", "S")
                        .on_click(cx.listener(|this, _, _, cx| this.skip_frames(None, cx))),
                )
                .child(
                    action("frames-duplicate", "Duplicate", "Ctrl+D").on_click(
                        cx.listener(|this, _, window, cx| this.duplicate_frames(window, cx)),
                    ),
                )
                .child(
                    action("frames-delete", "Delete", "Delete").on_click(
                        cx.listener(|this, _, window, cx| this.delete_frames(window, cx)),
                    ),
                ),
        )
        .child(div().text_xs().text_color(muted).child(match selected {
            0 => "Select frames in the strip.".to_string(),
            1 => "1 frame selected".to_string(),
            n => format!("{n} frames selected"),
        }))
        .into_any_element()
}
