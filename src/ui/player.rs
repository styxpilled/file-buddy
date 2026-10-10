//! The videos that may play: the clip in the side pane, the file opened ahead
//! of it, and the tile under the pointer. The pause and the press of the pane
//! and of the Studio work the same way, through `Transport`.

use std::path::Path;
use std::time::Duration;

use gpui::{Context, SharedString, Task};
use gpui_video::{Video, VideoOptions};
use url::Url;

use crate::media::Kind;
use crate::paths;
use crate::ui::FileBuddy;
use crate::ui::seek_bar::Clip;

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
/// A press on space or on the clip that lasts longer than this plays the clip
/// fast until the release. A shorter one is a tap, which toggles pause.
const HOLD_DELAY: Duration = Duration::from_millis(250);
/// The speed of a hold, and of the speed button when it is on.
const FAST: f64 = 2.0;

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
    /// 1x or 2x, from the speed button, for the pane and the Studio. Kept
    /// across selections, as the sound is. A hold plays at 2x whatever it is,
    /// and goes back to it.
    pub speed: f64,
    pub pane: Transport,
}

/// The pause of one clip, and what holds it: a drag on the bar, or space or
/// the mouse button down on the clip.
#[derive(Default)]
pub(super) struct Transport {
    pub paused: bool,
    /// A drag on the bar holds the clip paused. Pause and play wait for the
    /// release, which applies them.
    pub held: bool,
    press: Option<Press>,
    hold_task: Option<Task<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PressFrom {
    Key,
    Mouse,
}

struct Press {
    from: PressFrom,
    /// It lasted long enough to be a hold, and the clip plays fast.
    fast: bool,
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
            // Simulated checks run while the user works, so they are silent.
            muted: cfg!(feature = "sim"),
            volume: 0.5,
            autoplay: false,
            speed: 1.0,
            pane: Transport::new(true),
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
        video.set_speed(self.speed).ok();
        video.set_paused(self.pane.paused);
    }

    /// The level as the pane shows it. The pane strikes it through while the
    /// clip is muted, because the number alone would not explain the silence.
    pub fn volume_label(&self) -> SharedString {
        format!("{}%", (self.volume * 100.).round() as u32).into()
    }

    /// The speed a clip plays at now, for the speed button.
    pub fn speed_label(&self, transport: &Transport) -> &'static str {
        if self.speed_of(transport) != 1.0 {
            "2x"
        } else {
            "1x"
        }
    }

    fn speed_of(&self, transport: &Transport) -> f64 {
        if transport.holding() {
            FAST
        } else {
            self.speed
        }
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

impl Transport {
    pub fn new(paused: bool) -> Self {
        Self {
            paused,
            ..Self::default()
        }
    }

    /// Whether the clip plays now. A hold plays it whatever the pause says.
    pub fn playing(&self) -> bool {
        !self.paused || self.holding()
    }

    /// Returns false if a press is already down. Key repeat sends the action
    /// again, and it must not start a second press.
    fn press(&mut self, from: PressFrom) -> bool {
        if self.press.is_some() {
            return false;
        }
        self.press = Some(Press { from, fast: false });
        true
    }

    /// The press became a hold. `paused` stays what the user chose, so the
    /// release can go back to it.
    fn speed_up(&mut self) {
        if let Some(press) = &mut self.press {
            press.fast = true;
        }
    }

    /// A tap toggles pause. A hold goes back to the pause from before it.
    /// Returns whether a press ended.
    fn release(&mut self, from: PressFrom) -> bool {
        if self.press.as_ref().is_some_and(|press| press.from == from) {
            self.end_press(true);
            return true;
        }
        false
    }

    /// `tap` is false when the window lost focus, so a tap does nothing.
    fn end_press(&mut self, tap: bool) {
        self.hold_task = None;
        if let Some(press) = self.press.take()
            && !press.fast
            && tap
        {
            self.paused = !self.paused;
        }
    }

    /// A new clip. The press and the drag were for the old one.
    pub fn drop_press(&mut self) {
        self.press = None;
        self.hold_task = None;
        self.held = false;
    }

    fn holding(&self) -> bool {
        self.press.as_ref().is_some_and(|press| press.fast)
    }
}

impl FileBuddy {
    pub(super) fn transport(&mut self, clip: Clip) -> &mut Transport {
        match clip {
            Clip::Pane => &mut self.player.pane,
            Clip::Studio => &mut self.studio.transport,
        }
    }

    pub(super) fn transport_of(&self, clip: Clip) -> &Transport {
        match clip {
            Clip::Pane => &self.player.pane,
            Clip::Studio => &self.studio.transport,
        }
    }

    /// Pause or play the clip as the user chose. Nothing changes while a drag
    /// holds it. In the Studio, a clip that plays on from the out point starts
    /// again at the in point.
    pub(super) fn sync_paused(&self, clip: Clip) {
        let transport = self.transport_of(clip);
        let Some(video) = self.clip_video(clip) else {
            return;
        };
        if transport.held {
            return;
        }
        video.set_paused(!transport.playing());
        if clip == Clip::Studio && transport.playing() {
            self.loop_studio(video);
        }
    }

    pub(super) fn sync_speed(&self, clip: Clip) {
        if let Some(video) = self.clip_video(clip) {
            video.set_speed(self.clip_speed(clip)).ok();
        }
    }

    /// The speed button or a hold, and in the Studio the speed of the edit on
    /// top of it.
    pub(super) fn clip_speed(&self, clip: Clip) -> f64 {
        let speed = self.player.speed_of(self.transport_of(clip));
        match (clip, &self.studio.edit) {
            (Clip::Studio, Some(edit)) => speed * edit.speed,
            _ => speed,
        }
    }

    /// The sound is one for the pane and the Studio, so a change reaches both.
    fn sync_sound(&self) {
        for clip in [Clip::Pane, Clip::Studio] {
            if let Some(video) = self.clip_video(clip) {
                video.set_volume(self.player.volume);
                video.set_muted(self.player.muted);
            }
        }
    }

    pub(super) fn toggle_muted(&mut self, cx: &mut Context<Self>) {
        self.player.muted = !self.player.muted;
        self.sync_sound();
        cx.notify();
    }

    /// One step of the level. Turning it up is a request to hear the clip, so
    /// it lifts the mute.
    pub(super) fn nudge_volume(&mut self, up: bool, cx: &mut Context<Self>) {
        let player = &mut self.player;
        let step = if up { VOLUME_STEP } else { -VOLUME_STEP };
        player.volume = (player.volume + step).clamp(0., 1.);
        if up && player.volume > 0. {
            player.muted = false;
        }
        self.sync_sound();
        cx.notify();
    }

    /// The pause and the speed the user chose, on a clip the Studio opens.
    pub(super) fn apply_transport(&self, clip: Clip, video: &Video) {
        video.set_speed(self.clip_speed(clip)).ok();
        video.set_paused(!self.transport_of(clip).playing());
    }

    pub(super) fn toggle_paused(&mut self, clip: Clip, cx: &mut Context<Self>) {
        let transport = self.transport(clip);
        transport.paused = !transport.paused;
        self.sync_paused(clip);
        cx.notify();
    }

    /// The speed button, for both clips.
    pub(super) fn toggle_speed(&mut self, cx: &mut Context<Self>) {
        self.player.speed = if self.player.speed == 1.0 { FAST } else { 1.0 };
        self.sync_speed(Clip::Pane);
        self.sync_speed(Clip::Studio);
        cx.notify();
    }

    pub(super) fn toggle_autoplay(&mut self, cx: &mut Context<Self>) {
        self.player.autoplay = !self.player.autoplay;
        self.player.pane.paused = !self.player.autoplay;
        self.sync_paused(Clip::Pane);
        cx.notify();
    }

    /// Space down, or the mouse button down on the clip. Held past
    /// `HOLD_DELAY`, it plays the clip fast.
    pub(super) fn press_play(&mut self, clip: Clip, from: PressFrom, cx: &mut Context<Self>) {
        if !self.transport(clip).press(from) {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOLD_DELAY).await;
            this.update(cx, |this, cx| {
                this.transport(clip).speed_up();
                this.sync_speed(clip);
                this.sync_paused(clip);
                cx.notify();
            })
            .ok();
        });
        self.transport(clip).hold_task = Some(task);
    }

    pub(super) fn release_play(&mut self, from: PressFrom, cx: &mut Context<Self>) {
        for clip in [Clip::Pane, Clip::Studio] {
            if self.transport(clip).release(from) {
                self.sync_speed(clip);
                self.sync_paused(clip);
            }
        }
        cx.notify();
    }

    pub(super) fn pressed(&self) -> bool {
        [Clip::Pane, Clip::Studio]
            .iter()
            .any(|clip| self.transport_of(*clip).press.is_some())
    }

    /// The window lost focus, so no release will come. A hold ends, and a tap
    /// does nothing.
    pub(super) fn cancel_presses(&mut self, cx: &mut Context<Self>) {
        self.cancel_press(Clip::Pane);
        self.cancel_press(Clip::Studio);
        cx.notify();
    }

    /// The press became something else, such as a drag of the crop box.
    pub(super) fn cancel_press(&mut self, clip: Clip) {
        if self.transport_of(clip).press.is_some() {
            self.transport(clip).end_press(false);
            self.sync_speed(clip);
            self.sync_paused(clip);
        }
    }

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
        let moment = hit.at_ms;
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
        self.player.pane.drop_press();

        // The walk asks the user to name what they see, so the clip plays.
        self.player.pane.paused = !(self.player.autoplay || self.walking());

        // The file opened ahead is already decoded, so it costs no wait at all,
        // unless it must seek to a moment first.
        let preloaded = self
            .player
            .preloaded
            .take_if(|(at, _)| *at == self.selected)
            .map(|(_, video)| video);
        if let Some(video) = preloaded.clone()
            && moment.is_none()
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
            if preloaded.is_none() {
                cx.background_executor().timer(PREVIEW_DELAY).await;
            }
            let opened = this
                .update(cx, |this, _| {
                    preloaded
                        .or_else(|| this.open_clip(&path, &hash, PREVIEW_BUFFER))
                        .inspect(|video| {
                            this.player.apply(video);
                            open_at(video, moment);
                        })
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
            // A file that never decodes must not hold the old clip forever. A
            // clip opened at a moment shows its first frame until the seek
            // lands, so that frame is not shown.
            for _ in 0..FIRST_FRAME_TRIES {
                if video.frame_seq() > 0 && !video.seeking() {
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
        let hash = hit.file.hash.clone();
        self.player.preload_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PRELOAD_DELAY).await;
            this.update(cx, |this, _| {
                // The selection may have moved on while the timer ran.
                if this.next_index() != Some(index) {
                    return;
                }
                this.player.preloaded = this.open_clip(&path, &hash, PRELOAD_BUFFER).map(|video| {
                    video.set_muted(true);
                    video.set_paused(true);
                    (index, video)
                });
            })
            .ok();
        }));
    }

    pub(super) fn set_hovered(&mut self, index: usize, hovered: bool, cx: &mut Context<Self>) {
        if !self.autoscroll_hover(index, hovered) {
            return;
        }
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
            _ => self
                .open_clip(&hit.file.path, &hit.file.hash, PREVIEW_BUFFER)
                .map(|video| {
                    video.set_muted(true);
                    video.set_paused(false);
                    (index, video)
                }),
        };
        cx.notify();
    }

    /// A clip of `path`, unless a delete has its bytes in the shell.
    fn open_clip(&self, path: &Path, hash: &str, buffer: usize) -> Option<Video> {
        if self.deleting.iter().any(|deleting| deleting == hash) {
            return None;
        }
        open_video(&paths::playable(&self.cache, hash, path), buffer, true)
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

/// A match opens the clip where it matched. A clip already in the pane stays
/// where the user left it.
fn open_at(video: &Video, moment: Option<i64>) {
    if let Some(ms) = moment {
        video
            .seek(Duration::from_millis(ms.max(0) as u64), true)
            .ok();
    }
}

/// Open a file for playback in a tile, the side pane or the Studio. The caller
/// decides about sound and about pausing. A file FFmpeg cannot open gives
/// `None`, and the tile keeps its thumbnail. `buffer` caps the decoded frames it
/// holds, which matters because a frame is the full size of the clip.
pub(super) fn open_video(path: &Path, buffer: usize, looping: bool) -> Option<Video> {
    let uri = Url::from_file_path(path).ok()?;
    let options = VideoOptions {
        frame_buffer_capacity: buffer,
        looping,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_toggles_pause() {
        let mut transport = Transport::new(true);
        assert!(transport.press(PressFrom::Key));
        assert!(transport.release(PressFrom::Key));
        assert!(!transport.paused);
        assert!(!transport.holding());
    }

    #[test]
    fn a_hold_goes_back_to_the_state_from_before() {
        for paused in [true, false] {
            let mut transport = Transport::new(paused);
            transport.press(PressFrom::Mouse);
            transport.speed_up();
            assert!(transport.holding());
            assert!(transport.playing());
            transport.release(PressFrom::Mouse);
            assert_eq!(transport.paused, paused);
            assert!(!transport.holding());
        }
    }

    #[test]
    fn key_repeat_does_not_start_a_second_press() {
        let mut transport = Transport::new(true);
        assert!(transport.press(PressFrom::Key));
        assert!(!transport.press(PressFrom::Key));
        transport.release(PressFrom::Key);
        assert!(!transport.paused);
    }

    #[test]
    fn a_release_of_the_other_input_does_nothing() {
        let mut transport = Transport::new(true);
        transport.press(PressFrom::Key);
        assert!(!transport.release(PressFrom::Mouse));
        assert!(transport.paused);
        transport.release(PressFrom::Key);
        assert!(!transport.paused);
    }

    #[test]
    fn a_lost_focus_ends_a_hold_and_drops_a_tap() {
        let mut transport = Transport::new(true);
        transport.press(PressFrom::Key);
        transport.end_press(false);
        assert!(transport.paused);
        transport.press(PressFrom::Key);
        transport.speed_up();
        transport.end_press(false);
        assert!(transport.paused && !transport.holding());
    }

    #[test]
    fn the_speed_button_shows_the_speed_and_a_hold_goes_back_to_it() {
        let mut player = Player::default();
        let mut transport = Transport::new(false);
        assert_eq!(player.speed_label(&transport), "1x");
        transport.press(PressFrom::Key);
        transport.speed_up();
        assert_eq!(player.speed_label(&transport), "2x");
        transport.release(PressFrom::Key);
        assert_eq!(player.speed_label(&transport), "1x");
        player.speed = FAST;
        assert_eq!(player.speed_label(&transport), "2x");
    }
}
