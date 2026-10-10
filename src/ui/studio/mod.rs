//! The Studio page. It holds one edit: a source, the range cut from it, the
//! crop, the scale, the speed and the texts. The preview plays the range in a
//! loop at the speed, under the crop box and the texts. The timeline under it
//! holds the range and a row for each text, and the panel beside it holds the
//! export. A GIF or an animated WebP opens in frame mode, with a strip of its
//! frames over the timeline in place of the range. The edit stays until
//! another source replaces it. The clip closes while the page is not shown, so
//! it holds no file open.

mod strip;
mod text;

use std::path::Path;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, CursorStyle, DragMoveEvent, Entity, FocusHandle, Focusable, Hsla,
    MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollHandle, Subscription, Task, Window,
    black, div, img, px, relative, white,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, TextareaState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use gpui_video::{Video, video as video_element};

use crate::actions::clipboard;
use crate::ffmpeg::{export, probe};
use crate::media::{self, Kind};
use crate::paths;
use crate::studio::{self, Aspect, Controls, Crop, Edge, Edit, Format, Grip, MaxWidth, Quality};
use crate::ui::player::{Transport, open_video};
use crate::ui::seek_bar::{self, Clip, SeekBar};
use crate::ui::{
    AddText, DeleteText, DuplicateFrames, ExportEdit, FileBuddy, Page, SetIn, SetOut, header,
    preview, toggle,
};

const PANEL_WIDTH: f32 = 320.;
/// The In and Out buttons, one width for both.
const POINT_WIDTH: f32 = 40.;
const PADDING: f32 = 12.;
const BUFFER: usize = 120;
/// A grip on the crop box, square.
const GRIP_SIZE: f32 = 10.;
/// The input of a maximum width that is not a preset.
const WIDTH_INPUT: f32 = 56.;

pub(super) struct Studio {
    pub(super) edit: Option<Edit>,
    pub(super) video: Option<Video>,
    pub(super) seek_bar: SeekBar,
    /// The handle a drag holds.
    pub(super) handle: Option<Edge>,
    /// Held while the page is shown, so the Studio keys win.
    pub(super) focus: FocusHandle,
    /// Where the clip was when the page closed. It opens there again.
    resume: Duration,
    /// A drag on a handle holds the clip as a drag on the bar does.
    pub(super) transport: Transport,
    /// The share done of the export that runs. One runs at a time.
    export: Option<f32>,
    /// Apart from the edit, so a new source keeps them.
    controls: Controls,
    /// The grip a press on the crop box holds.
    crop_grab: Option<CropGrab>,
    /// A maximum width that is not a preset.
    pub(super) width_input: Entity<InputState>,
    _width_events: Subscription,
    pub(super) tab: Tab,
    /// The words of the selected text are an input in its row.
    pub(super) editing_text: bool,
    pub(super) text_scroll: ScrollHandle,
    /// The text the panel edits and `delete` removes.
    pub(super) selected_text: Option<usize>,
    text_grab: Option<text::TextGrab>,
    /// A drag on the middle of a text's bar: the text, where the press was,
    /// and the text's time then.
    pub(super) text_move: Option<(usize, Pixels, (u64, u64))>,
    pub(super) words_input: Entity<TextareaState>,
    _words_events: Subscription,
    pub(in crate::ui) strip: strip::Strip,
    /// The frames of a GIF or a WebP, while they decode.
    opening: Option<Task<()>>,
}

/// The tabs of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    Edit,
    Text,
}

impl Tab {
    const ALL: [(Tab, &str); 2] = [(Tab::Edit, "Edit"), (Tab::Text, "Text")];
}

struct CropGrab {
    grip: Grip,
    from: Point<Pixels>,
    crop: Crop,
    /// The pointer moved, so the press is a drag and not a tap on the clip.
    dragging: bool,
}

/// A grip of the crop box, while a drag holds it.
#[derive(Clone, Copy)]
struct CropDrag;

impl Render for CropDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

impl Studio {
    pub(super) fn new(window: &mut Window, cx: &mut Context<FileBuddy>) -> Self {
        let width_input = cx.new(|cx| InputState::new(window, cx).placeholder("px"));
        let width_events = cx.subscribe_in(
            &width_input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let typed = input.read(cx).value().trim().parse::<u32>();
                    if let (Ok(width @ 2..), Some(edit)) = (typed, &mut this.studio.edit) {
                        edit.max_width = MaxWidth::Pixels(width);
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } => window.focus(&this.studio.focus, cx),
                _ => {}
            },
        );
        let (words_input, words_events) = text::words_input(window, cx);
        Self {
            strip: strip::Strip::new(window, cx),
            opening: None,
            tab: Tab::Edit,
            editing_text: false,
            text_scroll: ScrollHandle::new(),
            selected_text: None,
            text_grab: None,
            text_move: None,
            words_input,
            _words_events: words_events,
            crop_grab: None,
            width_input,
            _width_events: width_events,
            edit: None,
            video: None,
            seek_bar: SeekBar::new(Clip::Studio, cx),
            handle: None,
            focus: cx.focus_handle(),
            resume: Duration::ZERO,
            transport: Transport::new(false),
            export: None,
            controls: Controls::default(),
        }
    }
}

enum Progress {
    Share(f32),
    Done(anyhow::Result<()>),
}

impl FileBuddy {
    /// `c`. The selection replaces the edit, as a new history step.
    pub(super) fn open_in_studio(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.selected_hit() else {
            return;
        };
        let still = hit.file.kind == Kind::Still;
        let animated = hit.file.kind == Kind::Animated;
        let hash = hit.file.hash.clone();
        let probed_ms = hit.file.duration.map(|seconds| (seconds * 1000.) as u64);
        let Some(path) = self.selected_file(cx) else {
            return;
        };
        self.studio.opening = None;
        // Frame mode is for GIF and WebP. An animated PNG opens as a video.
        let framed = animated
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("gif") || e.eq_ignore_ascii_case("webp"));
        if framed {
            self.open_frames(path, window, cx);
            return;
        }
        let (edit, video) = if still {
            let size = match probe::probe(&path, Kind::Still) {
                Ok(probe) => (probe.width, probe.height),
                Err(e) => {
                    log::warn!("cannot probe {}: {e:#}", path.display());
                    (None, None)
                }
            };
            let (Some(width @ 1..), Some(height @ 1..)) = size else {
                self.flash("this image cannot open", true, cx);
                return;
            };
            (Edit::still(path, (width as u32, height as u32)), None)
        } else {
            let input = paths::playable(&self.cache, &hash, &path);
            let Some(video) = open_video(&input, BUFFER, false) else {
                self.flash("this file cannot play", true, cx);
                return;
            };
            let duration_ms = match video.duration().as_millis() as u64 {
                0 => probed_ms.unwrap_or(0),
                ms => ms,
            };
            let (width, height) = video.size();
            let size = (width.max(0) as u32, height.max(0) as u32);
            (Edit::new(path, input, duration_ms, size), Some(video))
        };
        self.start_edit(edit, video, window, cx);
        self.reset_strip(Default::default(), window, cx);
    }

    /// The new edit replaces the old one, as a new history step.
    fn start_edit(
        &mut self,
        edit: Edit,
        video: Option<Video>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.record_step(cx);
        self.studio.controls.suit(edit.still);
        self.studio.edit = Some(edit);
        self.studio.crop_grab = None;
        self.studio.text_grab = None;
        self.studio.text_move = None;
        self.select_text(None, window, cx);
        self.studio
            .width_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.studio.resume = Duration::ZERO;
        self.studio.transport = Transport::new(false);
        if let Some(video) = &video {
            self.apply_studio(video);
        }
        self.studio.video = video;
        self.enter_page(Page::Studio, None, window, cx);
    }

    /// The page opens. The clip of the edit opens where it was.
    pub(super) fn open_studio_clip(&mut self) {
        if self.studio.video.is_some() {
            return;
        }
        let Some(edit) = self
            .studio
            .edit
            .as_ref()
            .filter(|edit| !edit.still && !edit.framed())
        else {
            return;
        };
        if let Some(video) = open_video(&edit.input, BUFFER, false) {
            self.apply_studio(&video);
            self.studio.video = Some(video);
        }
    }

    /// The page closes.
    pub(super) fn close_studio_clip(&mut self) {
        if let Some(video) = self.studio.video.take() {
            self.studio.resume = video.position();
        }
        self.studio.handle = None;
        self.studio.crop_grab = None;
        self.studio.text_grab = None;
        self.studio.text_move = None;
        self.studio.transport.drop_press();
        self.studio.strip.stop();
    }

    fn release_grabs(&mut self) {
        self.studio.crop_grab = None;
        self.studio.text_grab = None;
    }

    /// A press on the crop box or on one of its grips.
    fn grab_crop(&mut self, grip: Grip, from: Point<Pixels>) {
        if let Some(edit) = &self.studio.edit {
            self.studio.crop_grab = Some(CropGrab {
                grip,
                from,
                crop: edit.crop,
                dragging: false,
            });
        }
    }

    /// The pointer moves with a grip held. The first move ends the press on
    /// the clip, so a drag of the box does not also pause it.
    fn drag_crop(&mut self, to: Point<Pixels>, stage: Bounds<Pixels>, cx: &mut Context<Self>) {
        let (Some(edit), Some(grab)) = (&mut self.studio.edit, &mut self.studio.crop_grab) else {
            return;
        };
        let size = (f32::from(stage.size.width), f32::from(stage.size.height));
        let Some(frame) = frame_in(edit.size, size) else {
            return;
        };
        let dx = f32::from(to.x - grab.from.x) / frame.scale;
        let dy = f32::from(to.y - grab.from.y) / frame.scale;
        edit.drag_crop(grab.grip, grab.crop, f64::from(dx), f64::from(dy));
        let first = !std::mem::replace(&mut grab.dragging, true);
        if first {
            self.cancel_press(Clip::Studio);
        }
        cx.notify();
    }

    fn set_aspect(&mut self, aspect: Aspect, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.studio.edit {
            edit.set_aspect(aspect);
            cx.notify();
        }
    }

    fn set_max_width(&mut self, max_width: MaxWidth, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.studio.edit {
            edit.max_width = max_width;
            self.studio
                .width_input
                .update(cx, |input, cx| input.set_value("", window, cx));
            cx.notify();
        }
    }

    fn set_speed(&mut self, speed: f64, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.studio.edit {
            edit.speed = speed;
            self.sync_speed(Clip::Studio);
            cx.notify();
        }
    }

    fn apply_studio(&self, video: &Video) {
        video.set_muted(self.player.muted);
        video.set_volume(self.player.volume);
        self.apply_transport(Clip::Studio, video);
        video.seek(self.studio.resume, true).ok();
    }

    /// A clip at the out point, or at its end, starts again at the in point.
    pub(super) fn loop_studio(&self, video: &Video) {
        if let Some(edit) = &self.studio.edit
            && (video.ended() || video.position() >= Duration::from_millis(edit.range.end_ms))
        {
            video
                .seek(Duration::from_millis(edit.range.start_ms), true)
                .ok();
        }
    }

    /// `[` and `]`: a point of the range at the playhead.
    fn set_point(&mut self, edge: Edge, cx: &mut Context<Self>) {
        let (Some(edit), Some(video)) = (&mut self.studio.edit, &self.studio.video) else {
            return;
        };
        let at = video.position().as_millis() as u64;
        match edge {
            Edge::Start => edit.set_start(at),
            Edge::End => edit.set_end(at),
        }
        cx.notify();
    }

    /// Playback loops over the range. Called on every frame of the page.
    pub(super) fn step_studio(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page() != Page::Studio {
            return;
        }
        self.sync_seek_bar(Clip::Studio, window, cx);
        // Enter, escape or a click elsewhere took the keys from the words.
        if self.studio.editing_text
            && !self
                .studio
                .words_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            self.studio.editing_text = false;
            cx.notify();
        }
        self.step_strip(window, cx);
        let Some(video) = &self.studio.video else {
            return;
        };
        if !self.studio.transport.playing()
            || self.studio.transport.held
            || self.studio.seek_bar.dragging.is_some()
            || video.seeking()
        {
            return;
        }
        self.loop_studio(video);
    }

    /// `ctrl-enter`. FFmpeg runs on a thread of its own, and the page shows how
    /// far it is.
    fn export_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.studio.edit.clone() else {
            return;
        };
        if self.studio.export.is_some() {
            self.flash("an export is already running", true, cx);
            return;
        }
        let Some(folder) = self.config.export_folder.clone() else {
            self.flash("set an export folder in Settings", true, cx);
            return;
        };
        if !edit.still && edit.range.len_ms() == 0 {
            self.flash("the range is empty", true, cx);
            return;
        }
        let format = self.studio.controls.format;
        let (min_width, min_height) = format.min_size();
        let (width, height) = edit.output_size();
        if edit.size.0 > 0 && (width < min_width || height < min_height) {
            self.flash(
                format!(
                    "{} needs a frame of {min_width} x {min_height} or more",
                    format.label()
                ),
                true,
                cx,
            );
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&folder) {
            log::error!("create {}: {e}", folder.display());
            self.flash("export failed", true, cx);
            return;
        }
        let controls = self.studio.controls;
        let destination = studio::unique_path(
            &folder,
            &studio::export_name(&edit.source, edit.range, controls.format),
        );

        let (sender, mut events) = mpsc::unbounded();
        let target = destination.clone();
        std::thread::spawn(move || {
            let result = export::run(&edit, &controls, &target, |share| {
                sender.unbounded_send(Progress::Share(share)).ok();
            });
            sender.unbounded_send(Progress::Done(result)).ok();
        });
        self.studio.export = Some(0.);
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let open = this.update(cx, |this, cx| match event {
                    Progress::Share(share) => {
                        this.studio.export = Some(share);
                        cx.notify();
                    }
                    Progress::Done(result) => this.finish_export(&destination, result, cx),
                });
                if open.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// `p`. The frame on screen goes to the clipboard as an image, with no
    /// file: in the Studio with the crop and the texts and no scale, elsewhere
    /// the raw frame of the selection at its own size.
    pub(super) fn screenshot(&mut self, cx: &mut Context<Self>) {
        let position = |video: Option<&Video>| video.map_or(0, |v| v.position().as_millis() as u64);
        let (edit, at_ms) = if self.page() == Page::Studio {
            let Some(edit) = self.studio.edit.clone() else {
                return;
            };
            (edit, self.studio_at_ms())
        } else {
            let Some(hit) = self.selected_hit() else {
                return;
            };
            let still = hit.file.kind == Kind::Still;
            let hash = hit.file.hash.clone();
            let at_ms = position(self.player.video_of(&hash));
            let Some(path) = self.selected_file(cx) else {
                return;
            };
            let edit = if still {
                Edit::still(path, (0, 0))
            } else {
                let input = paths::playable(&self.cache, &hash, &path);
                Edit::new(path, input, 0, (0, 0))
            };
            (edit, at_ms)
        };
        cx.spawn(async move |this, cx| {
            let copied = cx
                .background_executor()
                .spawn(async move {
                    export::screenshot(&edit, at_ms).and_then(|png| clipboard::copy_image(&png))
                })
                .await;
            this.update(cx, |this, cx| match copied {
                Ok((width, height)) => {
                    this.flash(format!("copied the frame, {width} x {height}"), false, cx)
                }
                Err(e) => {
                    log::error!("screenshot failed: {e:#}");
                    this.flash(format!("screenshot failed: {e}"), true, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The file goes on the clipboard, as Enter puts a result there.
    fn finish_export(&mut self, path: &Path, result: anyhow::Result<()>, cx: &mut Context<Self>) {
        self.studio.export = None;
        if let Err(e) = result {
            log::error!("export to {} failed: {e:#}", path.display());
            self.flash("export failed", true, cx);
            return;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        match clipboard::copy_file(path) {
            Ok(()) => self.flash(
                format!("exported {}, copied {name}", media::bytes(size)),
                false,
                cx,
            ),
            Err(e) => {
                log::error!("copy of {} failed: {e:#}", path.display());
                self.flash(format!("exported {name}, copy failed: {e}"), true, cx);
            }
        }
    }
}

/// A point in the range, rounded to the tenth of a second as the length is:
/// `0:03.2`.
fn point(ms: u64) -> String {
    let tenths = (ms + 50) / 100;
    format!(
        "{}.{}",
        media::clock((tenths / 10 * 1000) as i64),
        tenths % 10
    )
}

pub(super) fn page(this: &FileBuddy, window: &Window, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(edit) = &this.studio.edit else {
        return v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child("Select a clip and press C to open it here.")
            .into_any_element();
    };
    let viewport = window.viewport_size();
    let width = (f32::from(viewport.width) - PANEL_WIDTH - PADDING * 2.).max(160.);
    let rows = seek_bar::TEXT_ROWS_HEIGHT;
    let timeline = if edit.still {
        0.
    } else if edit.framed() {
        strip::HEIGHT + seek_bar::HEIGHT + rows + 16.
    } else {
        seek_bar::HEIGHT + rows + 8.
    };
    let chrome = header::HEIGHT + timeline + PADDING * 2.;
    let height = (f32::from(viewport.height) - chrome).max(120.);

    h_flex()
        .id("studio")
        .key_context("Studio")
        .track_focus(&this.studio.focus)
        .size_full()
        .on_action(cx.listener(|this, _: &SetIn, _, cx| this.set_point(Edge::Start, cx)))
        .on_action(cx.listener(|this, _: &SetOut, _, cx| this.set_point(Edge::End, cx)))
        .on_action(cx.listener(|this, _: &ExportEdit, _, cx| this.export_edit(cx)))
        .on_action(cx.listener(|this, _: &AddText, window, cx| this.add_text(window, cx)))
        .on_action(cx.listener(|this, _: &DeleteText, window, cx| {
            if this.framed() && !this.studio.strip.selected.is_empty() {
                this.delete_frames(window, cx)
            } else {
                this.delete_text(window, cx)
            }
        }))
        .on_action(
            cx.listener(|this, _: &DuplicateFrames, window, cx| this.duplicate_frames(window, cx)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .p(px(PADDING))
                .child(stage(this, width, height, cx))
                .when(edit.framed(), |column| {
                    column.child(strip::strip(this, edit, width, cx))
                })
                .when(!edit.still, |column| {
                    column
                        .child(seek_bar::bar(this, Clip::Studio, width, cx))
                        .child(div().h(px(rows)).flex_shrink_0())
                }),
        )
        .child(panel(this, edit, cx))
        .into_any_element()
}

fn stage(this: &FileBuddy, width: f32, height: f32, cx: &mut Context<FileBuddy>) -> AnyElement {
    let frame = div()
        .id("studio-stage")
        .relative()
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .rounded(px(4.))
        .bg(cx.theme().secondary)
        .flex()
        .items_center()
        .justify_center();
    preview::clip_input(frame, Clip::Studio, cx)
        .on_drag_move(cx.listener(|this, event: &DragMoveEvent<CropDrag>, _, cx| {
            this.drag_crop(event.event.position, event.bounds, cx)
        }))
        .on_drag_move(
            cx.listener(|this, event: &DragMoveEvent<text::TextDrag>, _, cx| {
                this.drag_text(event.event.position, event.bounds, cx)
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.release_grabs()),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.release_grabs()),
        )
        .children(this.studio.video.as_ref().map(|video| {
            video_element(video.clone())
                .id("studio-preview")
                .size(px(width), px(height))
                .buffer_capacity(60)
                .into_any_element()
        }))
        .children(still_image(this, width, height))
        .children(strip::stage_frame(this, width, height))
        .children(crop_box(this, width, height, cx))
        .into_any_element()
}

/// A still, letterboxed as the video element does it, so the crop box lines up.
fn still_image(this: &FileBuddy, width: f32, height: f32) -> Option<AnyElement> {
    let edit = this.studio.edit.as_ref().filter(|edit| edit.still)?;
    let frame = frame_in(edit.size, (width, height))?;
    Some(
        img(edit.input.clone())
            .w(px(edit.size.0 as f32 * frame.scale))
            .h(px(edit.size.1 as f32 * frame.scale))
            .into_any_element(),
    )
}

/// Where the video letterboxes the frame in the stage, in stage pixels.
struct Frame {
    x: f32,
    y: f32,
    scale: f32,
}

fn frame_in((width, height): (u32, u32), (stage_width, stage_height): (f32, f32)) -> Option<Frame> {
    if width == 0 || height == 0 {
        return None;
    }
    let (width, height) = (width as f32, height as f32);
    let scale = (stage_width / width).min(stage_height / height);
    Some(Frame {
        x: (stage_width - width * scale) / 2.,
        y: (stage_height - height * scale) / 2.,
        scale,
    })
}

/// The crop over the preview. Outside the box the frame is dark. A drag inside
/// the box moves it, and a drag on a grip resizes it.
fn crop_box(
    this: &FileBuddy,
    width: f32,
    height: f32,
    cx: &mut Context<FileBuddy>,
) -> Option<AnyElement> {
    let edit = this.studio.edit.as_ref()?;
    if !edit.still && !edit.framed() {
        this.studio.video.as_ref()?;
    }
    let frame = frame_in(edit.size, (width, height))?;
    let crop = edit.crop;
    let frame_width = edit.size.0 as f32 * frame.scale;
    let frame_height = edit.size.1 as f32 * frame.scale;
    let left = frame.x + crop.x as f32 * frame.scale;
    let top = frame.y + crop.y as f32 * frame.scale;
    let box_width = crop.width as f32 * frame.scale;
    let box_height = crop.height as f32 * frame.scale;
    let right = left + box_width;
    let bottom = top + box_height;

    let shade = |x: f32, y: f32, w: f32, h: f32| {
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w.max(0.)))
            .h(px(h.max(0.)))
            .bg(black().opacity(0.6))
    };
    // A box on the whole frame crops nothing, so it stays in the background.
    let uncropped = crop == Crop::full(edit.size);
    let grip_fill = if uncropped {
        gpui::hsla(0., 0., 0.45, 1.)
    } else {
        white()
    };
    let texts = text::layer(this, edit, (left, top, box_width, box_height), cx);
    let grips = Grip::EDGES.into_iter().enumerate().map(|(at, grip)| {
        let (along, down) = grip.anchor();
        div()
            .id(("crop-grip", at))
            .absolute()
            .left(px(left + along * box_width - GRIP_SIZE / 2.))
            .top(px(top + down * box_height - GRIP_SIZE / 2.))
            .size(px(GRIP_SIZE))
            .bg(grip_fill)
            .border_1()
            .border_color(black())
            .cursor(grip_cursor(grip))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    // The stage under it would take the press as a tap.
                    cx.stop_propagation();
                    this.grab_crop(grip, event.position);
                }),
            )
            .on_drag(CropDrag, |drag, _, _, cx| cx.new(|_| *drag))
    });
    let border: Hsla = white().opacity(if uncropped { 0.3 } else { 0.8 });

    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(shade(frame.x, frame.y, frame_width, top - frame.y))
            .child(shade(
                frame.x,
                bottom,
                frame_width,
                frame.y + frame_height - bottom,
            ))
            .child(shade(frame.x, top, left - frame.x, box_height))
            .child(shade(right, top, frame.x + frame_width - right, box_height))
            .child(
                div()
                    .id("crop-box")
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(box_width))
                    .h(px(box_height))
                    .border_1()
                    .border_color(border)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, _| {
                            this.grab_crop(Grip::Move, event.position)
                        }),
                    )
                    .on_drag(CropDrag, |drag, _, _, cx| cx.new(|_| *drag)),
            )
            .child(texts)
            .children(grips)
            .into_any_element(),
    )
}

fn grip_cursor(grip: Grip) -> CursorStyle {
    match grip {
        Grip::Left | Grip::Right => CursorStyle::ResizeLeftRight,
        Grip::Top | Grip::Bottom => CursorStyle::ResizeUpDown,
        Grip::TopLeft | Grip::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        Grip::TopRight | Grip::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
        Grip::Move => CursorStyle::Arrow,
    }
}

fn panel(this: &FileBuddy, edit: &Edit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let playback = preview::controls(this, Clip::Studio, cx);
    let texts = text::panel(this, edit, cx);
    let frames = edit.framed().then(|| strip::panel(this, edit, cx));
    let theme = cx.theme();
    let name = edit
        .source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let range = edit.range;
    let muted = |text: String| div().text_color(theme.muted_foreground).child(text);
    // A click sets the point at the playhead, as its key does.
    let point_button = |edge: Edge, label: &'static str, key: &'static str| {
        Button::new(label)
            .xsmall()
            .w(px(POINT_WIDTH))
            .label(label)
            .tooltip(key)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.set_point(edge, cx)))
    };
    let length = |ms: u64| media::duration(ms as f64 / 1000.);
    let running = this.studio.export;
    let controls = this.studio.controls;

    let formats: &[Format] = if edit.still {
        &Format::STILL
    } else {
        &Format::MOVING
    };
    let range_row = (!edit.still && !edit.framed()).then(|| {
        h_flex()
            .gap_1p5()
            .items_center()
            .whitespace_nowrap()
            .child(point_button(Edge::Start, "In", "["))
            .child(point(range.start_ms))
            .child(muted("-".into()))
            .child(point(range.end_ms))
            .child(point_button(Edge::End, "Out", "]"))
            .child(div().flex_1())
            .child(length(edit.output_ms()))
            .child(muted(format!("/ {}", length(edit.duration_ms))))
    });
    let speed = (!edit.still).then(|| {
        v_flex()
            .gap_1()
            .child(div().text_color(theme.muted_foreground).child("Speed"))
            .child(
                h_flex()
                    .gap_1()
                    .children(studio::SPEEDS.into_iter().enumerate().map(|(at, speed)| {
                        toggle(
                            ("speed", at),
                            studio::speed_label(speed),
                            edit.speed == speed,
                        )
                        .xsmall()
                        .on_click(cx.listener(move |this, _, _, cx| this.set_speed(speed, cx)))
                    })),
            )
    });

    let main = v_flex()
        .gap_4()
        .child(div().child(name))
        .children(range_row)
        .children(frames)
        .child(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .child(
                            div()
                                .flex_1()
                                .text_color(theme.muted_foreground)
                                .child("Crop"),
                        )
                        .child(
                            Button::new("crop-reset")
                                .xsmall()
                                .label("Reset")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(edit) = &mut this.studio.edit {
                                        edit.reset_crop();
                                        cx.notify();
                                    }
                                })),
                        ),
                )
                .child(h_flex().gap_1().children(Aspect::ALL.map(|aspect| {
                    toggle(
                        ("aspect", aspect as usize),
                        aspect.label(),
                        edit.aspect == aspect,
                    )
                    .xsmall()
                    .on_click(cx.listener(move |this, _, _, cx| this.set_aspect(aspect, cx)))
                }))),
        )
        .child(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .child(
                            div()
                                .flex_1()
                                .text_color(theme.muted_foreground)
                                .child("Max width"),
                        )
                        .child(div().text_xs().text_color(theme.muted_foreground).child({
                            let (width, height) = edit.output_size();
                            format!("{width} x {height}")
                        })),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            toggle("width-source", "Source", edit.max_width == MaxWidth::Source)
                                .xsmall()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_max_width(MaxWidth::Source, window, cx)
                                })),
                        )
                        .children(studio::MAX_WIDTHS.map(|width| {
                            let max_width = MaxWidth::Pixels(width);
                            toggle(
                                ("width", width as usize),
                                width.to_string(),
                                edit.max_width == max_width
                                    && this.studio.width_input.read(cx).value().is_empty(),
                            )
                            .xsmall()
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.set_max_width(max_width, window, cx)
                                },
                            ))
                        }))
                        .child(
                            div()
                                .w(px(WIDTH_INPUT))
                                .child(Input::new(&this.studio.width_input).xsmall()),
                        ),
                ),
        )
        .children(speed)
        .child(
            v_flex()
                .gap_1()
                .child(div().text_color(theme.muted_foreground).child("Format"))
                .child(h_flex().gap_1().children(formats.iter().map(|&format| {
                    toggle(
                        ("format", format as usize),
                        format.label(),
                        controls.format == format,
                    )
                    .xsmall()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.studio.controls.format = format;
                        cx.notify();
                    }))
                }))),
        )
        .when_some(
            controls.fps().filter(|_| !edit.framed()),
            |panel, current| {
                panel.child(
                    v_flex()
                        .gap_1()
                        .child(div().text_color(theme.muted_foreground).child("Frame rate"))
                        .child(h_flex().gap_1().children(studio::FPS.map(|fps| {
                            toggle(("fps", fps as usize), fps.to_string(), current == fps)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.studio.controls.set_fps(fps);
                                    cx.notify();
                                }))
                        }))),
                )
            },
        )
        .when_some(controls.quality(), |panel, current| {
            panel.child(
                v_flex()
                    .gap_1()
                    .child(div().text_color(theme.muted_foreground).child("Quality"))
                    .child(h_flex().gap_1().children(Quality::ALL.map(|quality| {
                        toggle(quality.label(), quality.label(), current == quality)
                            .xsmall()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.studio.controls.set_quality(quality);
                                cx.notify();
                            }))
                    }))),
            )
        })
        .child(
            h_flex().child(
                Button::new("export")
                    .small()
                    .primary()
                    .label("Export")
                    .tooltip("Ctrl+Enter")
                    .disabled(running.is_some())
                    .when(running.is_none(), |button| button.cursor_pointer())
                    .on_click(cx.listener(|this, _, _, cx| this.export_edit(cx))),
            ),
        )
        .when_some(running, |panel, share| {
            panel.child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(format!("Exporting, {}%", (share * 100.).round() as u32)),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(4.))
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
            )
        })
        .when(this.config.export_folder.is_none(), |panel| {
            panel.child(
                div()
                    .text_xs()
                    .text_color(theme.warning)
                    .child("No export folder. Set one in Settings."),
            )
        });
    let tab = this.studio.tab;
    let tabs = h_flex().gap_1().children(Tab::ALL.map(|(shown, label)| {
        toggle(("studio-tab", shown as usize), label, tab == shown)
            .xsmall()
            .on_click(cx.listener(move |this, _, window, cx| {
                // An input of the other tab must not keep the keys.
                window.focus(&this.studio.focus, cx);
                this.studio.tab = shown;
                cx.notify();
            }))
    }));

    v_flex()
        .w(px(PANEL_WIDTH))
        .h_full()
        .flex_shrink_0()
        .gap_4()
        .p(px(PADDING))
        .border_l_1()
        .border_color(theme.border)
        .text_sm()
        .child(playback)
        .child(tabs)
        .child(match tab {
            Tab::Edit => main.into_any_element(),
            Tab::Text => texts,
        })
        .into_any_element()
}
