//! The Studio page. It holds one edit: a source and the range cut from it. The
//! preview plays the range in a loop, the timeline under it holds the range,
//! and the panel beside it holds the export. The edit stays until another
//! source replaces it. The clip closes while the page is not shown, so it holds
//! no file open.

use std::path::Path;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, FocusHandle, MouseButton, ScrollDelta, ScrollWheelEvent, Window, div, px,
    relative,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use gpui_video::{Video, video as video_element};

use crate::actions::clipboard;
use crate::ffmpeg::export;
use crate::media::{self, Kind};
use crate::paths;
use crate::studio::{self, Edge, Edit, Format, Quality};
use crate::ui::player::open_video;
use crate::ui::seek_bar::{self, Clip, SeekBar};
use crate::ui::{ExportEdit, FileBuddy, Page, SetIn, SetOut, header, toggle};

const PANEL_WIDTH: f32 = 300.;
const PADDING: f32 = 12.;
const BUFFER: usize = 120;

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
    paused: bool,
    /// The share done of the export that runs. One runs at a time.
    export: Option<f32>,
}

impl Studio {
    pub(super) fn new(cx: &mut Context<FileBuddy>) -> Self {
        Self {
            edit: None,
            video: None,
            seek_bar: SeekBar::new(Clip::Studio, cx),
            handle: None,
            focus: cx.focus_handle(),
            resume: Duration::ZERO,
            paused: false,
            export: None,
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
        if hit.file.kind == Kind::Still {
            self.flash("the Studio does not take stills yet", true, cx);
            return;
        }
        let hash = hit.file.hash.clone();
        let probed_ms = hit.file.duration.map(|seconds| (seconds * 1000.) as u64);
        let Some(path) = self.selected_file(cx) else {
            return;
        };
        let input = paths::playable(&self.cache, &hash, &path);
        let Some(video) = open_video(&input, BUFFER, false) else {
            self.flash("this file cannot play", true, cx);
            return;
        };
        let duration_ms = match video.duration().as_millis() as u64 {
            0 => probed_ms.unwrap_or(0),
            ms => ms,
        };
        self.record_step(cx);
        self.studio.edit = Some(Edit::new(path, input, duration_ms));
        self.studio.resume = Duration::ZERO;
        self.studio.paused = false;
        self.apply_studio(&video);
        self.studio.video = Some(video);
        self.enter_page(Page::Studio, None, window, cx);
    }

    /// The page opens. The clip of the edit opens where it was.
    pub(super) fn open_studio_clip(&mut self) {
        if self.studio.video.is_some() {
            return;
        }
        let Some(edit) = &self.studio.edit else {
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
    }

    fn apply_studio(&self, video: &Video) {
        video.set_muted(self.player.muted);
        video.set_volume(self.player.volume);
        video.set_paused(self.studio.paused);
        video.seek(self.studio.resume, true).ok();
    }

    /// The sound is the one the side pane has, so a change reaches both.
    pub(super) fn sync_studio_sound(&self) {
        if let Some(video) = &self.studio.video {
            video.set_muted(self.player.muted);
            video.set_volume(self.player.volume);
        }
    }

    pub(super) fn toggle_studio_paused(&mut self, cx: &mut Context<Self>) {
        let Some(video) = &self.studio.video else {
            return;
        };
        self.studio.paused = !self.studio.paused;
        video.set_paused(self.studio.paused);
        // A clip at its end starts again at the in point, not at zero.
        if !self.studio.paused
            && let Some(edit) = &self.studio.edit
            && (video.ended() || video.position() >= Duration::from_millis(edit.range.end_ms))
        {
            video
                .seek(Duration::from_millis(edit.range.start_ms), true)
                .ok();
        }
        cx.notify();
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
        let (Some(edit), Some(video)) = (&self.studio.edit, &self.studio.video) else {
            return;
        };
        if self.studio.paused
            || self.studio.handle.is_some()
            || self.studio.seek_bar.dragging.is_some()
            || video.seeking()
        {
            return;
        }
        if video.ended() || video.position() >= Duration::from_millis(edit.range.end_ms) {
            video
                .seek(Duration::from_millis(edit.range.start_ms), true)
                .ok();
        }
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
        if edit.range.len_ms() == 0 {
            self.flash("the range is empty", true, cx);
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&folder) {
            log::error!("create {}: {e}", folder.display());
            self.flash("export failed", true, cx);
            return;
        }
        let format = Format::Mp4;
        let destination = studio::unique_path(
            &folder,
            &studio::export_name(&edit.source, edit.range, format),
        );

        let (sender, mut events) = mpsc::unbounded();
        let target = destination.clone();
        std::thread::spawn(move || {
            let result = export::run(&edit, format, &target, |share| {
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

    fn set_quality(&mut self, quality: Quality, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.studio.edit {
            edit.quality = quality;
            cx.notify();
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
    let height =
        (f32::from(viewport.height) - header::HEIGHT - seek_bar::HEIGHT - 8. - PADDING * 2.)
            .max(120.);

    h_flex()
        .id("studio")
        .key_context("Studio")
        .track_focus(&this.studio.focus)
        .size_full()
        .on_action(cx.listener(|this, _: &SetIn, _, cx| this.set_point(Edge::Start, cx)))
        .on_action(cx.listener(|this, _: &SetOut, _, cx| this.set_point(Edge::End, cx)))
        .on_action(cx.listener(|this, _: &ExportEdit, _, cx| this.export_edit(cx)))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .p(px(PADDING))
                .child(stage(this, width, height, cx))
                .child(seek_bar::bar(this, Clip::Studio, width, cx)),
        )
        .child(panel(this, edit, cx))
        .into_any_element()
}

/// A click pauses or plays the clip, and the wheel sets the sound level.
fn stage(this: &FileBuddy, width: f32, height: f32, cx: &mut Context<FileBuddy>) -> AnyElement {
    div()
        .id("studio-stage")
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .rounded(px(4.))
        .bg(cx.theme().secondary)
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.toggle_studio_paused(cx)),
        )
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
            let up = match event.delta {
                ScrollDelta::Lines(delta) => delta.y,
                ScrollDelta::Pixels(delta) => f32::from(delta.y),
            };
            if up != 0. {
                this.player.nudge_volume(up > 0.);
                this.sync_studio_sound();
                cx.notify();
            }
        }))
        .children(this.studio.video.as_ref().map(|video| {
            video_element(video.clone())
                .id("studio-preview")
                .size(px(width), px(height))
                .buffer_capacity(60)
        }))
        .into_any_element()
}

fn panel(this: &FileBuddy, edit: &Edit, cx: &mut Context<FileBuddy>) -> AnyElement {
    let theme = cx.theme();
    let name = edit
        .source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let range = edit.range;
    let label = |text: &'static str| {
        div()
            .w(px(48.))
            .text_color(theme.muted_foreground)
            .child(text)
    };
    let point_row = |edge: Edge, text: &'static str, button: &'static str, key: &'static str| {
        h_flex()
            .gap_2()
            .items_center()
            .child(label(text))
            .child(div().flex_1().child(point(range.at(edge))))
            .child(
                Button::new(button)
                    .xsmall()
                    .label(button)
                    .tooltip(key)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.set_point(edge, cx))),
            )
    };
    let running = this.studio.export;

    v_flex()
        .w(px(PANEL_WIDTH))
        .h_full()
        .flex_shrink_0()
        .gap_4()
        .p(px(PADDING))
        .border_l_1()
        .border_color(theme.border)
        .text_sm()
        .child(div().child(name))
        .child(
            v_flex()
                .gap_1()
                .child(point_row(Edge::Start, "In", "Set in", "["))
                .child(point_row(Edge::End, "Out", "Set out", "]"))
                .child(
                    h_flex()
                        .gap_2()
                        .child(label("Length"))
                        .child(media::duration(range.len_ms() as f64 / 1000.)),
                ),
        )
        .child(
            v_flex()
                .gap_1()
                .child(div().text_color(theme.muted_foreground).child("Format"))
                .child(
                    h_flex()
                        .gap_1()
                        .child(toggle("format-mp4", "MP4", true).xsmall()),
                ),
        )
        .child(
            v_flex()
                .gap_1()
                .child(div().text_color(theme.muted_foreground).child("Quality"))
                .child(h_flex().gap_1().children(Quality::ALL.map(|quality| {
                    toggle(quality.label(), quality.label(), edit.quality == quality)
                        .xsmall()
                        .on_click(cx.listener(move |this, _, _, cx| this.set_quality(quality, cx)))
                }))),
        )
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
        .child(div().text_xs().map(|line| {
            match &this.config.export_folder {
                Some(folder) => line
                    .text_color(theme.muted_foreground)
                    .child(format!("To {}", folder.display())),
                None => line
                    .text_color(theme.warning)
                    .child("No export folder. Set one in Settings."),
            }
        }))
        .into_any_element()
}
