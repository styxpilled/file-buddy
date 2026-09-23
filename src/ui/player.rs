//! The videos that may play: the clip in the side pane, the file opened ahead
//! of it, and the tile under the pointer.

use std::path::Path;
use std::time::Duration;

use gpui::{Context, SharedString, Task};
use gpui_video::{Video, VideoOptions};
use url::Url;

use crate::media::Kind;
use crate::ui::FileBuddy;

/// How long the selection must rest before the side pane opens a decoder. Held
/// arrow keys would otherwise open one file per keystroke.
const PREVIEW_DELAY: Duration = Duration::from_millis(180);
/// How long the pane waits for the first frame of the new clip before it shows
/// it anyway. `FIRST_FRAME_TRIES * FIRST_FRAME_POLL` is the whole budget.
const FIRST_FRAME_POLL: Duration = Duration::from_millis(16);
const FIRST_FRAME_TRIES: usize = 75;
/// How long the next file in the direction of travel waits before it opens. It
/// must outlast `PREVIEW_DELAY`, so a held arrow key opens nothing at all.
const PRELOAD_DELAY: Duration = Duration::from_millis(400);
/// Frames the file opened ahead may hold. It is paused, so it stops at the
/// prebuffer anyway, but a cap keeps a large clip from taking the memory of a
/// full buffer for a file the user may never reach.
const PRELOAD_BUFFER: usize = 8;
const PREVIEW_BUFFER: usize = 120;
/// One notch of the wheel, or one press of `ctrl-up`.
const VOLUME_STEP: f64 = 0.05;

pub(super) struct Player {
    preview: Option<Preview>,
    preview_task: Option<Task<()>>,
    /// The next file in the direction of travel, opened silent and still, with
    /// the index it was opened for. A change of direction discards it.
    preloaded: Option<(usize, Video)>,
    preload_task: Option<Task<()>>,
    /// The last step through the grid, which says which file to open ahead.
    pub step: isize,
    /// The one tile that plays. Only one does.
    pub hovered: Option<(usize, Video)>,
    /// Both kept across selections, so the sound does not come back by itself
    /// and does not return to full.
    pub muted: bool,
    pub volume: f64,
    /// Whether a new preview starts playing. Pausing one clip does not change
    /// it, and it never touches a tile.
    pub autoplay: bool,
    pub paused: bool,
}

/// The clip in the side pane, with the identity of what it holds. Identity is
/// the content hash, the same as everywhere else in the application, so two
/// paths with the same bytes are one video and stepping between them does not
/// start the clip again.
struct Preview {
    hash: String,
    video: Video,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            preview: None,
            preview_task: None,
            preloaded: None,
            preload_task: None,
            step: 1,
            hovered: None,
            muted: false,
            volume: 0.5,
            autoplay: false,
            paused: true,
        }
    }
}

impl Player {
    pub fn video(&self) -> Option<&Video> {
        self.preview.as_ref().map(|preview| &preview.video)
    }

    /// The clip in the pane, if it holds these bytes.
    pub fn video_of(&self, hash: &str) -> Option<&Video> {
        self.preview
            .as_ref()
            .filter(|preview| preview.hash == hash)
            .map(|preview| &preview.video)
    }

    /// The sound and the pause the user chose, on a clip the pane takes.
    fn apply(&self, video: &Video) {
        video.set_muted(self.muted);
        video.set_volume(self.volume);
        video.set_paused(self.paused);
    }

    pub fn toggle_muted(&mut self) {
        self.muted = !self.muted;
        if let Some(video) = self.video() {
            video.set_muted(self.muted);
        }
    }

    /// One step of the level. Turning it up is a request to hear the clip, so
    /// it lifts the mute.
    pub fn nudge_volume(&mut self, up: bool) {
        let step = if up { VOLUME_STEP } else { -VOLUME_STEP };
        self.volume = (self.volume + step).clamp(0., 1.);
        if up && self.volume > 0. {
            self.muted = false;
        }
        if let Some(video) = self.video() {
            video.set_volume(self.volume);
            video.set_muted(self.muted);
        }
    }

    /// One clip only. The next selection follows the autoplay flag again.
    pub fn toggle_paused(&mut self) {
        self.paused = !self.paused;
        if let Some(video) = self.video() {
            video.set_paused(self.paused);
        }
    }

    pub fn toggle_autoplay(&mut self) {
        self.autoplay = !self.autoplay;
        self.paused = !self.autoplay;
        if let Some(video) = self.video() {
            video.set_paused(self.paused);
        }
    }

    /// The level as the header shows it. A muted clip says so, because the
    /// number alone would not explain the silence.
    pub fn volume_label(&self) -> SharedString {
        if self.muted {
            return "muted".into();
        }
        format!("{}%", (self.volume * 100.).round() as u32).into()
    }

    /// Keep the tile that plays and the file opened ahead only where `keep`
    /// says their row still holds the file they were opened for.
    pub fn retain(&mut self, keep: impl Fn(usize) -> bool) {
        if self.hovered.as_ref().is_some_and(|(at, _)| !keep(*at)) {
            self.hovered = None;
        }
        if self.preloaded.as_ref().is_some_and(|(at, _)| !keep(*at)) {
            self.preloaded = None;
        }
    }
}

impl FileBuddy {
    /// The side pane plays the selection, after it stops moving. The clip on
    /// screen stays there, silent, until the new one has a frame to show.
    /// Clearing it first left the stage empty for a moment, which flashed.
    pub(super) fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        self.player.preview_task = None;
        let Some(hit) = self.selected_hit() else {
            self.player.preview = None;
            self.player.preloaded = None;
            return;
        };
        let kind = hit.file.kind;
        let path = hit.file.path.clone();
        let hash = hit.file.hash.clone();
        // A still image has no clip of its own, so nothing may linger over it.
        if kind == Kind::Still {
            self.player.preview = None;
            self.schedule_preload(cx);
            return;
        }

        // Already playing this one. A search runs on every keystroke and puts
        // the same file back under the selection, and a step onto a duplicate
        // lands on the same bytes, so reopening here would throw away a warm
        // decoder and start the clip again from nothing for no reason at all.
        // Whether it is playing and how loud it is are the user's answers to a
        // question about this clip, so they stand too.
        if self.player.video_of(&hash).is_some() {
            self.schedule_preload(cx);
            return;
        }

        // The walk asks the user to name what they see, so the clip plays.
        self.player.paused = !(self.player.autoplay || self.walking());

        // The file opened ahead is already decoded, so it costs no wait at all.
        if let Some((_, video)) = self
            .player
            .preloaded
            .take_if(|(at, _)| *at == self.selected)
        {
            self.player.apply(&video);
            self.player.preview = Some(Preview { hash, video });
            self.schedule_preload(cx);
            cx.notify();
            return;
        }

        if let Some(outgoing) = self.player.video() {
            outgoing.set_muted(true);
        }
        let index = self.selected;
        self.player.preview_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DELAY).await;
            let opened = this
                .update(cx, |this, _| {
                    open_video(&path, PREVIEW_BUFFER).inspect(|video| this.player.apply(video))
                })
                .ok()
                .flatten();
            let Some(video) = opened else {
                // FFmpeg cannot open it, so the thumbnail is all there is.
                this.update(cx, |this, cx| {
                    if this.selected == index {
                        this.player.preview = None;
                        this.schedule_preload(cx);
                        cx.notify();
                    }
                })
                .ok();
                return;
            };
            // A file that never decodes must not hold the old clip forever.
            for _ in 0..FIRST_FRAME_TRIES {
                if video.frame_seq() > 0 {
                    break;
                }
                cx.background_executor().timer(FIRST_FRAME_POLL).await;
            }
            this.update(cx, |this, cx| {
                if this.selected != index {
                    return;
                }
                this.player.preview = Some(Preview { hash, video });
                this.schedule_preload(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// The file one step ahead, if the grid holds one.
    fn next_index(&self) -> Option<usize> {
        let ahead = usize::try_from(self.selected as isize + self.player.step).ok()?;
        (ahead < self.results.len()).then_some(ahead)
    }

    /// Open the next file in the direction of travel, so the arrow key that
    /// reaches it has nothing to wait for. It stays silent and still until
    /// then, which costs one decoder and a few frames.
    fn schedule_preload(&mut self, cx: &mut Context<Self>) {
        self.player.preload_task = None;
        let Some(index) = self.next_index() else {
            self.player.preloaded = None;
            return;
        };
        if matches!(&self.player.preloaded, Some((at, _)) if *at == index) {
            return;
        }
        self.player.preloaded = None;
        let Some(hit) = self.hit_at(index) else {
            return;
        };
        if hit.file.kind == Kind::Still {
            return;
        }
        let path = hit.file.path.clone();
        self.player.preload_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PRELOAD_DELAY).await;
            this.update(cx, |this, _| {
                // The selection may have moved on while the timer ran.
                if this.next_index() != Some(index) {
                    return;
                }
                this.player.preloaded = open_video(&path, PRELOAD_BUFFER).map(|video| {
                    video.set_muted(true);
                    video.set_paused(true);
                    (index, video)
                });
            })
            .ok();
        }));
    }

    pub(super) fn set_hovered(&mut self, index: usize, hovered: bool, cx: &mut Context<Self>) {
        let playing = self.player.hovered.as_ref().map(|(at, _)| *at);
        if !hovered {
            if playing == Some(index) {
                self.player.hovered = None;
                cx.notify();
            }
            return;
        }
        if playing == Some(index) {
            return;
        }
        let Some(hit) = self.hit_at(index) else {
            return;
        };
        // A still image does not react to hover. A tile is always silent.
        self.player.hovered = match hit.file.kind {
            Kind::Still => None,
            _ => open_video(&hit.file.path, PREVIEW_BUFFER).map(|video| {
                video.set_muted(true);
                video.set_paused(false);
                (index, video)
            }),
        };
        cx.notify();
    }

    /// Close every clip that has these bytes open. FFmpeg opens a file without
    /// delete sharing, so Windows refuses a rename while one of them plays.
    pub(super) fn release_videos(&mut self, hash: &str) {
        if self.player.video_of(hash).is_some() {
            self.player.preview = None;
            self.player.preview_task = None;
        }
        let results = &self.results;
        self.player.retain(|at| {
            results
                .get(at)
                .and_then(|entry| entry.hit())
                .is_none_or(|hit| hit.file.hash != hash)
        });
    }

    /// Close the tile that plays and the file opened ahead. Both were opened
    /// for a row of results that are gone.
    pub(super) fn stop_videos(&mut self) {
        self.player.hovered = None;
        self.player.preloaded = None;
    }
}

/// Open a file for playback in a tile or in the side pane. It loops; the caller
/// decides about sound and about pausing. A file FFmpeg cannot open gives
/// `None`, and the tile keeps its thumbnail. `buffer` caps the decoded frames it
/// holds, which matters because a frame is the full size of the clip.
fn open_video(path: &Path, buffer: usize) -> Option<Video> {
    let uri = Url::from_file_path(path).ok()?;
    let options = VideoOptions {
        frame_buffer_capacity: buffer,
        looping: true,
        ..VideoOptions::default()
    };
    match Video::new_with_options(&uri, options) {
        Ok(video) => Some(video),
        Err(e) => {
            log::warn!("cannot play {}: {e}", path.display());
            None
        }
    }
}
