//! Frame mode in the Studio: the strip of frames over the timeline, each with
//! its delay, the frame the stage shows, and playback by the delays. A click
//! selects a frame, shift-click a run and ctrl-click one more, and a drag
//! moves the selected frames.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, DragMoveEvent, Entity, Hsla, Modifiers, MouseButton,
    MouseDownEvent, ObjectFit, Render, RenderImage, ScrollHandle, Subscription, Window, div, img,
    px, rgb,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use image::RgbaImage;

use super::frame_in;
use crate::studio::Edit;
use crate::studio::frames::{self, Frame, MIN_DELAY_MS};
use crate::ui::FileBuddy;
use crate::ui::seek_bar::{self, Clip};

const CELL_WIDTH: f32 = 64.;
const CELL_GAP: f32 = 4.;
const THUMB_HEIGHT: f32 = 40.;
/// The small copies are this many pixels high, so they stay sharp at 200%.
const THUMB_PIXELS: u32 = 80;
const DELAY_LINE: f32 = 16.;
/// The room for the scrollbar under the cells.
const SCROLLBAR: f32 = 10.;
/// The height of the strip on the page.
pub(super) const HEIGHT: f32 = THUMB_HEIGHT + DELAY_LINE + SCROLLBAR + 4.;
/// The input of the delay.
const DELAY_INPUT: f32 = 64.;

pub(in crate::ui) struct Strip {
    pub(super) selected: BTreeSet<usize>,
    /// Where the last plain click was. Shift-click selects from here.
    anchor: usize,
    /// The playhead, in milliseconds of the strip.
    at_ms: f64,
    /// When the playhead last moved, while the frames play.
    clock: Option<Instant>,
    /// The small copies of the strip, by image. A copy of a frame shares one.
    thumbs: HashMap<usize, Arc<RenderImage>>,
    /// The frame on the stage, at full size, by image.
    shown: Option<(usize, Arc<RenderImage>)>,
    /// The gap the drag would put the selected frames in.
    drop_gap: Option<usize>,
    /// A drag just moved the frames, so its release is no click.
    moved: bool,
    scroll: ScrollHandle,
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

/// The frames of a file and their small copies, made away from the window.
pub(super) struct Decoded {
    pub(super) frames: Vec<Frame>,
    thumbs: HashMap<usize, Arc<RenderImage>>,
}

pub(super) fn decode(path: &std::path::Path) -> anyhow::Result<Decoded> {
    let frames = frames::decode(path)?;
    let mut thumbs = HashMap::new();
    for frame in &frames {
        thumbs.entry(key(frame)).or_insert_with(|| {
            let (width, height) = frame.image.dimensions();
            let thumb_width = (width * THUMB_PIXELS / height.max(1)).max(1);
            let small = image::imageops::thumbnail(frame.image.as_ref(), thumb_width, THUMB_PIXELS);
            render_image(&small)
        });
    }
    Ok(Decoded { frames, thumbs })
}

fn key(frame: &Frame) -> usize {
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
            thumbs: HashMap::new(),
            shown: None,
            drop_gap: None,
            moved: false,
            scroll: ScrollHandle::new(),
            delay_input,
            _delay_events: delay_events,
        }
    }

    pub(in crate::ui) fn at_ms(&self) -> u64 {
        self.at_ms as u64
    }

    /// The page closed. The clock starts again when the frames play.
    pub(super) fn stop(&mut self) {
        self.clock = None;
        self.drop_gap = None;
    }
}

impl FileBuddy {
    /// A new source. The images of the last one leave the atlas.
    pub(super) fn reset_strip(
        &mut self,
        thumbs: HashMap<usize, Arc<RenderImage>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strip = &mut self.studio.strip;
        let old: Vec<_> = std::mem::replace(&mut strip.thumbs, thumbs)
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
        strip.clock = None;
        strip.drop_gap = None;
        strip.moved = false;
        self.sync_delay_input(window, cx);
    }

    /// Opens a GIF or an animated WebP in frame mode, once the frames are
    /// decoded away from the window.
    pub(super) fn open_frames(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flash("opening the frames", false, cx);
        let task = cx.spawn_in(window, async move |this, cx| {
            let source = path.clone();
            let decoded = cx
                .background_executor()
                .spawn(async move { decode(&source) })
                .await;
            this.update_in(cx, |this, window, cx| match decoded {
                Ok(decoded) => {
                    this.studio.opening = None;
                    this.notice = None;
                    let edit = Edit::with_frames(path, decoded.frames);
                    this.start_edit(edit, None, window, cx);
                    this.reset_strip(decoded.thumbs, window, cx);
                }
                Err(e) => {
                    this.studio.opening = None;
                    log::warn!("cannot decode {}: {e:#}", path.display());
                    this.flash("this file cannot open", true, cx);
                }
            })
            .ok();
        });
        self.studio.opening = Some(task);
    }

    pub(in crate::ui) fn framed(&self) -> bool {
        self.studio.edit.as_ref().is_some_and(Edit::framed)
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
    }

    /// Called on every frame of the page. The playhead runs at the speed of
    /// the edit and the speed button, and the stage holds the frame under it.
    pub(super) fn step_strip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.studio.edit.as_ref().filter(|edit| edit.framed()) else {
            return;
        };
        let playing = self.studio.transport.playing() && self.studio.seek_bar.dragging.is_none();
        let length = edit.duration_ms.max(1) as f64;
        let speed = self.clip_speed(Clip::Studio);
        let strip = &mut self.studio.strip;
        if playing {
            let now = Instant::now();
            if let Some(then) = strip.clock {
                strip.at_ms = (strip.at_ms + (now - then).as_secs_f64() * 1000. * speed) % length;
            }
            strip.clock = Some(now);
            window.request_animation_frame();
        } else {
            strip.clock = None;
        }

        let index = edit.frame_at(strip.at_ms as u64);
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
            strip.scroll.scroll_to_item(index);
        }
    }

    /// A press on a frame. Shift and ctrl change the selection at once. A
    /// press on a frame already selected waits for the release, so a drag can
    /// move the whole selection.
    fn press_frame(
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
    fn click_frame(
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
        self.studio.strip.scroll.scroll_to_item(first);
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
        if strip.drop_gap != Some(gap) {
            strip.drop_gap = Some(gap);
            cx.notify();
        }
    }

    fn drop_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

/// Every frame with its delay, in a row that scrolls. The frame under the
/// playhead has its delay in the accent color.
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
    let cells = edit.frames.iter().enumerate().map(|(index, frame)| {
        let selected = strip.selected.contains(&index);
        let gap_before = strip.drop_gap == Some(index);
        div()
            .id(("strip-frame", index))
            .relative()
            .flex_shrink_0()
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
                    .children(strip.thumbs.get(&key(frame)).map(|thumb| {
                        img(thumb.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                    })),
            )
            .child(
                div()
                    .h(px(DELAY_LINE))
                    .text_xs()
                    .text_center()
                    .text_color(if index == current { accent } else { muted })
                    .child(format!("{} ms", frame.delay_ms)),
            )
            .when(gap_before, |cell| {
                cell.child(gap_marker(accent, -CELL_GAP / 2. - 1.))
            })
            .when(
                strip.drop_gap == Some(edit.frames.len()) && index + 1 == edit.frames.len(),
                |cell| cell.child(gap_marker(accent, CELL_WIDTH + CELL_GAP / 2. - 1.)),
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
                .gap(px(CELL_GAP))
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
                .children(cells),
        )
        .horizontal_scrollbar(&strip.scroll)
        .into_any_element()
}

fn gap_marker(color: Hsla, left: f32) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left(px(left))
        .w(px(2.))
        .h(px(THUMB_HEIGHT))
        .bg(color)
}

/// The panel in frame mode: the count and length, and the delay of the
/// selected frames with the keys that copy and remove them.
pub(super) fn panel(this: &FileBuddy, edit: &Edit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let selected = this.studio.strip.selected.len();
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let length = |ms: u64| crate::media::duration(ms as f64 / 1000.);
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .child(
                    div()
                        .flex_1()
                        .child(format!("{} frames", edit.frames.len())),
                )
                .child(length(edit.output_ms()))
                .child(
                    div()
                        .text_color(muted)
                        .ml_1()
                        .child(format!("/ {}", length(edit.duration_ms))),
                ),
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
                    Button::new("frames-duplicate")
                        .xsmall()
                        .label("Duplicate")
                        .tooltip("Ctrl+D")
                        .disabled(selected == 0)
                        .when(selected > 0, |button| button.cursor_pointer())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.duplicate_frames(window, cx)),
                        ),
                )
                .child(
                    Button::new("frames-delete")
                        .xsmall()
                        .label("Delete")
                        .tooltip("Delete")
                        .disabled(selected == 0)
                        .when(selected > 0, |button| button.cursor_pointer())
                        .on_click(
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
