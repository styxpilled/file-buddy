//! Frame mode: every frame of a GIF or an animated WebP in memory, each with
//! its delay, and the edits of the strip. Text times are in output time, so a
//! text stays at its moment when frames go or move. The range is a first and a
//! last frame, and a skipped frame stays on the strip with no time.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, RgbaImage};

use super::sequence::Place;
use super::{Edge, Edit, Range};

/// Browsers play a delay this short or shorter at `SHORT_DELAY_PLAYS_AT`.
const SHORT_DELAY_MS: u32 = 10;
const SHORT_DELAY_PLAYS_AT: u32 = 100;
/// The shortest delay the panel takes. A GIF keeps delays in hundredths of a
/// second, and a browser plays 10 ms or less at 100 ms.
pub const MIN_DELAY_MS: u32 = 20;

/// One frame of the strip. Copies share the image.
#[derive(Clone)]
pub struct Frame {
    pub image: Arc<RgbaImage>,
    pub delay_ms: u32,
    /// It does not play and does not export.
    pub skipped: bool,
    /// Its move and scale on the canvas of a sequence.
    pub place: Place,
}

impl Frame {
    pub fn new(image: RgbaImage, delay_ms: u32) -> Self {
        Self {
            image: Arc::new(image),
            delay_ms,
            skipped: false,
            place: Place::default(),
        }
    }

    /// Its time on the strip.
    pub fn length_ms(&self) -> u64 {
        if self.skipped {
            0
        } else {
            u64::from(self.delay_ms)
        }
    }
}

impl PartialEq for Frame {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.image, &other.image)
            && self.delay_ms == other.delay_ms
            && self.skipped == other.skipped
            && self.place == other.place
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("size", &self.image.dimensions())
            .field("delay_ms", &self.delay_ms)
            .field("skipped", &self.skipped)
            .finish()
    }
}

/// The delay as a browser plays it.
pub fn played(delay_ms: u32) -> u32 {
    if delay_ms <= SHORT_DELAY_MS {
        SHORT_DELAY_PLAYS_AT
    } else {
        delay_ms
    }
}

/// Every frame of a GIF or a WebP, whole and in RGBA.
pub fn decode(path: &Path) -> Result<Vec<Frame>> {
    let mut frames = Vec::new();
    decode_each(path, |frame| {
        frames.push(frame);
        true
    })?;
    Ok(frames)
}

/// The frames of a GIF or a WebP in order, one at a time, as they decode.
/// `each` returns false to stop.
pub fn decode_each(path: &Path, mut each: impl FnMut(Frame) -> bool) -> Result<()> {
    let mut head = [0u8; 12];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .with_context(|| format!("read {}", path.display()))?;
    let mut count = 0;
    let mut counted = |frame| {
        count += 1;
        each(frame)
    };
    if head.starts_with(b"GIF8") {
        gif(path, &mut counted)?;
    } else if &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        webp(path, &mut counted)?;
    } else {
        bail!("not a GIF or a WebP");
    }
    if count == 0 {
        bail!("the animation has no frames");
    }
    Ok(())
}

fn gif(path: &Path, each: &mut impl FnMut(Frame) -> bool) -> Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let decoder = GifDecoder::new(BufReader::new(file)).context("decode the GIF")?;
    for frame in decoder.into_frames() {
        let frame = frame.context("decode a frame")?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay_ms = played(numerator / denominator.max(1));
        if !each(Frame::new(frame.into_buffer(), delay_ms)) {
            break;
        }
    }
    Ok(())
}

fn webp(path: &Path, each: &mut impl FnMut(Frame) -> bool) -> Result<()> {
    let mut decoder = crate::ffmpeg::webp::open(path)?;
    let (width, height) = decoder.dimensions();
    let alpha = decoder.has_alpha();
    let len = decoder
        .output_buffer_size()
        .context("the frames are too large")?;
    let mut buffer = vec![0; len];
    let rgba = |buffer: &[u8]| -> Result<RgbaImage> {
        let pixels = if alpha {
            buffer.to_vec()
        } else {
            buffer
                .chunks_exact(3)
                .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                .collect()
        };
        RgbaImage::from_raw(width, height, pixels).context("a frame of the wrong size")
    };
    if !decoder.is_animated() {
        decoder
            .read_image(&mut buffer)
            .context("decode the image")?;
        each(Frame::new(rgba(&buffer)?, SHORT_DELAY_PLAYS_AT));
        return Ok(());
    }
    for _ in 0..decoder.num_frames() {
        let delay = decoder.read_frame(&mut buffer).context("decode a frame")?;
        if !each(Frame::new(rgba(&buffer)?, played(delay))) {
            break;
        }
    }
    Ok(())
}

/// A click on the strip. Shift selects the run from the last plain click, and
/// ctrl adds or removes one frame.
pub fn click(
    selected: &mut BTreeSet<usize>,
    anchor: &mut usize,
    index: usize,
    shift: bool,
    ctrl: bool,
) {
    if shift {
        *selected = (index.min(*anchor)..=index.max(*anchor)).collect();
        return;
    }
    if ctrl {
        if !selected.remove(&index) {
            selected.insert(index);
        }
    } else {
        *selected = BTreeSet::from([index]);
    }
    *anchor = index;
}

impl Edit {
    /// Frame mode, with the whole strip as the range.
    pub fn with_frames(source: PathBuf, frames: Vec<Frame>) -> Self {
        let size = frames
            .first()
            .map_or((0, 0), |frame| frame.image.dimensions());
        let mut edit = Self {
            frames,
            frame_range: (0, usize::MAX),
            ..Self::new(source.clone(), source, 0, size)
        };
        edit.retime();
        edit
    }

    pub fn framed(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Frames that decode after the edit opens go at the end. A range on the
    /// whole strip grows with them.
    pub fn push_frames(&mut self, frames: Vec<Frame>) {
        let whole = self.whole_range();
        self.frames.extend(frames);
        if whole {
            self.frame_range.1 = usize::MAX;
        }
        self.retime();
    }

    fn whole_range(&self) -> bool {
        self.frame_range == (0, self.frames.len().saturating_sub(1))
    }

    /// The length follows the strip, and the range follows its first and last
    /// frame.
    fn retime(&mut self) {
        self.duration_ms = self.frames.iter().map(Frame::length_ms).sum();
        let last = self.frames.len().saturating_sub(1);
        let (first, end) = self.frame_range;
        self.frame_range = (first.min(last), end.clamp(first.min(last), last));
        self.range = Range {
            start_ms: self.frame_start_ms(self.frame_range.0),
            end_ms: self.frame_start_ms(self.frame_range.1 + 1),
        };
    }

    pub fn frame_start_ms(&self, index: usize) -> u64 {
        self.frames[..index.min(self.frames.len())]
            .iter()
            .map(Frame::length_ms)
            .sum()
    }

    /// The frame that shows at `at_ms` of the strip. A skipped frame never
    /// shows, and the last shown frame holds past the end.
    pub fn frame_at(&self, at_ms: u64) -> usize {
        let mut end = 0;
        for (index, frame) in self.frames.iter().enumerate() {
            end += frame.length_ms();
            if at_ms < end {
                return index;
            }
        }
        self.frames
            .iter()
            .rposition(|frame| !frame.skipped)
            .unwrap_or(0)
    }

    /// The times of the gaps between frames, from 0 to the length of the
    /// strip, once each.
    fn gap_times(&self) -> Vec<u64> {
        let mut times = vec![0];
        let mut at = 0;
        for frame in &self.frames {
            at += frame.length_ms();
            if times.last() != Some(&at) {
                times.push(at);
            }
        }
        times
    }

    /// Where an end of a text goes in frame mode: the gap nearest `at_ms` on
    /// its side of the other end, `other_ms`. `None` when no gap is there.
    pub(super) fn text_edge_gap(&self, edge: Edge, at_ms: u64, other_ms: u64) -> Option<u64> {
        self.gap_times()
            .into_iter()
            .filter(|&gap| match edge {
                Edge::Start => gap < other_ms,
                Edge::End => gap > other_ms,
            })
            .min_by_key(|gap| gap.abs_diff(at_ms))
    }

    /// Where a moved text starts in frame mode: the gap nearest `at_ms`, no
    /// later than `latest_ms`.
    pub(super) fn text_start_gap(&self, at_ms: u64, latest_ms: u64) -> u64 {
        self.gap_times()
            .into_iter()
            .filter(|&gap| gap <= latest_ms)
            .min_by_key(|gap| gap.abs_diff(at_ms))
            .unwrap_or(0)
    }

    /// The gap between frames nearest `at_ms`, from 0 to the length of the
    /// strip.
    fn gap_near(&self, at_ms: u64) -> usize {
        let mut start = 0u64;
        let mut best = (u64::MAX, 0);
        for gap in 0..=self.frames.len() {
            best = best.min((start.abs_diff(at_ms), gap));
            if let Some(frame) = self.frames.get(gap) {
                start += frame.length_ms();
            }
        }
        best.1
    }

    /// The frames the export keeps: those in the range that are not skipped,
    /// each with its start on the strip.
    pub fn kept_frames(&self) -> impl Iterator<Item = (u64, &Frame)> {
        let (first, last) = self.frame_range;
        let frames = self.frames.get(first..=last).unwrap_or_default();
        frames
            .iter()
            .scan(self.range.start_ms, |start, frame| {
                let at = *start;
                *start += frame.length_ms();
                Some((at, frame))
            })
            .filter(|(_, frame)| !frame.skipped)
    }

    /// `[`. The range starts at the frame on the stage.
    pub(super) fn set_first_frame(&mut self, at_ms: u64) {
        let first = self.frame_at(at_ms);
        if first > self.frame_range.1 {
            self.frame_range.1 = usize::MAX;
        }
        self.frame_range.0 = first;
        self.retime();
    }

    /// `]`. The range ends with the frame on the stage.
    pub(super) fn set_last_frame(&mut self, at_ms: u64) {
        let last = self.frame_at(at_ms);
        if last < self.frame_range.0 {
            self.frame_range.0 = 0;
        }
        self.frame_range.1 = last;
        self.retime();
    }

    /// A drag on a handle snaps it to the gap between frames nearest it. It
    /// stops at the other handle, so the range keeps one frame.
    pub(super) fn drag_frame_edge(&mut self, edge: Edge, at_ms: u64) {
        let gap = self.gap_near(at_ms);
        let (first, last) = self.frame_range;
        self.frame_range = match edge {
            Edge::Start => (gap.min(last), last),
            Edge::End => (first, gap.max(first + 1) - 1),
        };
        self.retime();
    }

    /// Where the playhead goes for a handle. At the end it is the last frame
    /// of the range, not the frame after it.
    pub fn handle_ms(&self, edge: Edge) -> u64 {
        match edge {
            Edge::End if self.framed() => self.frame_start_ms(self.frame_range.1),
            _ => self.range.at(edge),
        }
    }

    /// `delete`. Returns the frame to select after it: the one after the
    /// first removed, or the last. An animation keeps one frame, so with every
    /// frame selected nothing goes and this returns `None`.
    pub fn delete_frames(&mut self, selected: &BTreeSet<usize>) -> Option<usize> {
        let len = self.frames.len();
        let count = selected.range(..len).count();
        if count == 0 || count == len {
            return None;
        }
        let first = *selected.first()?;
        let whole = self.whole_range();
        let mut index = 0;
        self.frames.retain(|_| {
            let keep = !selected.contains(&index);
            index += 1;
            keep
        });
        // The range keeps the frames of it that stay. With none of them left,
        // it is the whole strip.
        let (start, end) = self.frame_range;
        let new_start = start - selected.range(..start).count();
        let kept_to_end = end + 1 - selected.range(..=end).count();
        self.frame_range = if whole || kept_to_end <= new_start {
            (0, usize::MAX)
        } else {
            (new_start, kept_to_end - 1)
        };
        self.retime();
        Some(first.min(self.frames.len() - 1))
    }

    /// `ctrl-d`. The copies go after the last selected frame, in order, and
    /// become the selection. Inside the range they join it.
    pub fn duplicate_frames(&mut self, selected: &BTreeSet<usize>) -> BTreeSet<usize> {
        let copies: Vec<Frame> = selected
            .iter()
            .filter_map(|&index| self.frames.get(index).cloned())
            .collect();
        let Some(&last) = selected.range(..self.frames.len()).next_back() else {
            return BTreeSet::new();
        };
        let at = last + 1;
        let count = copies.len();
        let whole = self.whole_range();
        self.frames.splice(at..at, copies);
        let (start, end) = self.frame_range;
        self.frame_range = if whole {
            (0, usize::MAX)
        } else {
            (
                if at <= start { start + count } else { start },
                if at <= end + 1 { end + count } else { end },
            )
        };
        self.retime();
        (at..at + count).collect()
    }

    /// A drag puts the selected frames in the gap before `gap`, from 0 to the
    /// length of the strip. They keep their order and stay selected. The range
    /// follows its first and last frame.
    pub fn move_frames(&mut self, selected: &BTreeSet<usize>, gap: usize) -> BTreeSet<usize> {
        let gap = gap.min(self.frames.len());
        let whole = self.whole_range();
        let at = gap - selected.range(..gap).count();
        let count = selected.range(..self.frames.len()).count();
        let (mut moved, mut rest) = (Vec::new(), Vec::new());
        let new_index = |index: usize| {
            if selected.contains(&index) {
                at + selected.range(..index).count()
            } else {
                let before = index - selected.range(..index).count();
                if before < at { before } else { before + count }
            }
        };
        let (start, end) = self.frame_range;
        let (start, end) = (new_index(start), new_index(end));
        for (index, frame) in self.frames.drain(..).enumerate() {
            if selected.contains(&index) {
                moved.push(frame);
            } else {
                rest.push(frame);
            }
        }
        rest.splice(at..at, moved);
        self.frames = rest;
        self.frame_range = if whole {
            (0, usize::MAX)
        } else {
            (start.min(end), start.max(end))
        };
        self.retime();
        (at..at + count).collect()
    }

    /// `s`. The selected frames stop playing, or play again when all of them
    /// were skipped. One frame always plays, so a skip of every frame that
    /// plays does nothing and returns false.
    pub fn toggle_skip(&mut self, selected: &BTreeSet<usize>) -> bool {
        let chosen = || {
            self.frames
                .iter()
                .enumerate()
                .filter(|(index, _)| selected.contains(index))
        };
        let skip = chosen().any(|(_, frame)| !frame.skipped);
        let plays_on = self
            .frames
            .iter()
            .enumerate()
            .any(|(index, frame)| !frame.skipped && !selected.contains(&index));
        if chosen().next().is_none() || skip && !plays_on {
            return false;
        }
        for &index in selected {
            if let Some(frame) = self.frames.get_mut(index) {
                frame.skipped = skip;
            }
        }
        self.retime();
        true
    }

    pub fn set_delay(&mut self, selected: &BTreeSet<usize>, delay_ms: u32) {
        for &index in selected {
            if let Some(frame) = self.frames.get_mut(index) {
                frame.delay_ms = delay_ms.max(MIN_DELAY_MS);
            }
        }
        self.retime();
    }

    /// The delay all the selected frames share, if they share one.
    pub fn shared_delay(&self, selected: &BTreeSet<usize>) -> Option<u32> {
        let mut delays = selected
            .iter()
            .filter_map(|&index| self.frames.get(index))
            .map(|frame| frame.delay_ms);
        let first = delays.next()?;
        delays.all(|delay| delay == first).then_some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames told apart by their one pixel, `0` to `n - 1`, with delays.
    fn strip(delays: &[u32]) -> Edit {
        let frames = delays
            .iter()
            .enumerate()
            .map(|(at, &delay_ms)| {
                Frame::new(
                    RgbaImage::from_pixel(1, 1, image::Rgba([at as u8, 0, 0, 255])),
                    delay_ms,
                )
            })
            .collect();
        Edit::with_frames("a.gif".into(), frames)
    }

    /// Each frame as its first number, and its delay.
    fn order(edit: &Edit) -> Vec<(u8, u32)> {
        edit.frames
            .iter()
            .map(|frame| (frame.image.get_pixel(0, 0)[0], frame.delay_ms))
            .collect()
    }

    fn set(indices: &[usize]) -> BTreeSet<usize> {
        indices.iter().copied().collect()
    }

    #[test]
    fn the_strip_sets_the_length_and_the_frame_at_a_time() {
        let edit = strip(&[100, 50, 250]);
        assert_eq!(edit.duration_ms, 400);
        assert_eq!((edit.range.start_ms, edit.range.end_ms), (0, 400));
        assert_eq!(edit.size, (1, 1));
        assert_eq!(edit.frame_at(0), 0);
        assert_eq!(edit.frame_at(149), 1);
        assert_eq!(edit.frame_at(150), 2);
        assert_eq!(edit.frame_at(9_000), 2);
        assert_eq!(edit.frame_start_ms(2), 150);
    }

    #[test]
    fn a_delete_keeps_the_other_delays_and_one_frame() {
        let mut edit = strip(&[100, 50, 250, 30]);
        assert_eq!(edit.delete_frames(&set(&[1, 2])), Some(1));
        assert_eq!(order(&edit), [(0, 100), (3, 30)]);
        assert_eq!(edit.duration_ms, 130);
        assert_eq!(edit.range.end_ms, 130);
        assert_eq!(edit.delete_frames(&set(&[1])), Some(0));
        assert_eq!(edit.delete_frames(&set(&[0])), None);
        assert_eq!(order(&edit), [(0, 100)]);
    }

    #[test]
    fn a_duplicate_goes_after_the_last_selected_frame() {
        let mut edit = strip(&[100, 50, 250, 30]);
        assert_eq!(edit.duplicate_frames(&set(&[0, 2])), set(&[3, 4]));
        assert_eq!(
            order(&edit),
            [(0, 100), (1, 50), (2, 250), (0, 100), (2, 250), (3, 30)]
        );
        assert_eq!(edit.duration_ms, 780);
        assert!(Arc::ptr_eq(&edit.frames[0].image, &edit.frames[3].image));
        assert!(edit.duplicate_frames(&set(&[])).is_empty());
    }

    #[test]
    fn a_move_keeps_the_order_and_the_delays() {
        let mut edit = strip(&[10, 20, 30, 40, 50]);
        // Frames 1 and 3 go before frame 0.
        assert_eq!(edit.move_frames(&set(&[1, 3]), 0), set(&[0, 1]));
        assert_eq!(order(&edit), [(1, 20), (3, 40), (0, 10), (2, 30), (4, 50)]);
        // The first two go to the end.
        assert_eq!(edit.move_frames(&set(&[0, 1]), 5), set(&[3, 4]));
        assert_eq!(order(&edit), [(0, 10), (2, 30), (4, 50), (1, 20), (3, 40)]);
        // A gap inside the selection leaves the frames together there.
        assert_eq!(edit.move_frames(&set(&[0, 2]), 1), set(&[0, 1]));
        assert_eq!(order(&edit), [(0, 10), (4, 50), (2, 30), (1, 20), (3, 40)]);
        assert_eq!(edit.duration_ms, 150);
    }

    #[test]
    fn a_delay_changes_the_selected_frames_and_has_a_floor() {
        let mut edit = strip(&[100, 50, 250]);
        assert_eq!(edit.shared_delay(&set(&[0, 1])), None);
        edit.set_delay(&set(&[0, 1]), 70);
        assert_eq!(edit.shared_delay(&set(&[0, 1])), Some(70));
        assert_eq!(edit.duration_ms, 390);
        edit.set_delay(&set(&[2]), 5);
        assert_eq!(order(&edit)[2], (2, MIN_DELAY_MS));
    }

    #[test]
    fn a_click_selects_one_a_run_or_toggles() {
        let (mut selected, mut anchor) = (BTreeSet::new(), 0);
        click(&mut selected, &mut anchor, 2, false, false);
        assert_eq!(selected, set(&[2]));
        click(&mut selected, &mut anchor, 5, true, false);
        assert_eq!(selected, set(&[2, 3, 4, 5]));
        click(&mut selected, &mut anchor, 0, true, false);
        assert_eq!(selected, set(&[0, 1, 2]));
        click(&mut selected, &mut anchor, 1, false, true);
        assert_eq!(selected, set(&[0, 2]));
        click(&mut selected, &mut anchor, 7, false, true);
        assert_eq!(selected, set(&[0, 2, 7]));
        click(&mut selected, &mut anchor, 5, true, false);
        assert_eq!(selected, set(&[5, 6, 7]));
    }

    /// The first number of each frame the export keeps, with its start.
    fn kept(edit: &Edit) -> Vec<(u8, u64)> {
        edit.kept_frames()
            .map(|(at, frame)| (frame.image.get_pixel(0, 0)[0], at))
            .collect()
    }

    #[test]
    fn the_range_is_whole_frames_and_snaps_to_the_nearest_gap() {
        let mut edit = strip(&[100, 50, 250, 30]);
        // `[` and `]` take the frame on the stage.
        edit.set_start(120);
        edit.set_end(160);
        assert_eq!(edit.frame_range, (1, 2));
        assert_eq!((edit.range.start_ms, edit.range.end_ms), (100, 400));
        assert_eq!(kept(&edit), [(1, 100), (2, 150)]);
        assert_eq!(edit.handle_ms(Edge::End), 150);
        // An in point past the out point moves the out point to the end.
        edit.set_start(410);
        assert_eq!(edit.frame_range, (3, 3));
        edit.set_end(0);
        assert_eq!(edit.frame_range, (0, 0));
        // A handle snaps to the gap nearest it, and keeps one frame.
        edit.drag(Edge::End, 330);
        assert_eq!(edit.frame_range, (0, 2));
        edit.drag(Edge::Start, 110);
        assert_eq!(edit.frame_range, (1, 2));
        edit.drag(Edge::Start, 9_000);
        assert_eq!(edit.frame_range, (2, 2));
        edit.drag(Edge::End, 0);
        assert_eq!(edit.frame_range, (2, 2));
    }

    #[test]
    fn a_skipped_frame_has_no_time_and_is_not_kept() {
        let mut edit = strip(&[100, 50, 250, 30]);
        assert!(edit.toggle_skip(&set(&[1, 3])));
        assert_eq!(edit.duration_ms, 350);
        assert_eq!(edit.frame_at(100), 2);
        assert_eq!(edit.frame_at(9_000), 2);
        assert_eq!(edit.frame_start_ms(3), 350);
        assert_eq!(kept(&edit), [(0, 0), (2, 100)]);
        // A selection with one frame that plays is skipped whole.
        assert!(edit.toggle_skip(&set(&[0, 1])));
        assert_eq!(kept(&edit), [(2, 0)]);
        // The last frame that plays stays.
        assert!(!edit.toggle_skip(&set(&[2])));
        // All of them skipped play again.
        assert!(edit.toggle_skip(&set(&[0, 1, 3])));
        assert_eq!(edit.duration_ms, 430);
        assert!(!edit.toggle_skip(&set(&[])));
    }

    #[test]
    fn the_range_keeps_its_frames_through_the_edits() {
        let mut edit = strip(&[10, 20, 30, 40, 50]);
        edit.set_start(10);
        edit.set_end(30);
        assert_eq!(edit.frame_range, (1, 2));
        // A delete before the range moves it back.
        edit.delete_frames(&set(&[0]));
        assert_eq!(edit.frame_range, (0, 1));
        assert_eq!(kept(&edit), [(1, 0), (2, 20)]);
        // A copy inside the range joins it, one after it does not.
        edit.duplicate_frames(&set(&[1]));
        assert_eq!(edit.frame_range, (0, 2));
        edit.duplicate_frames(&set(&[3]));
        assert_eq!(edit.frame_range, (0, 2));
        // The range follows its first and last frame through a move.
        edit.move_frames(&set(&[3, 4]), 0);
        assert_eq!(edit.frame_range, (2, 4));
        assert_eq!(kept(&edit), [(1, 80), (2, 100), (2, 130)]);
        // With every frame of it gone, the range is the whole strip.
        edit.delete_frames(&set(&[2, 3, 4]));
        assert_eq!(edit.frame_range, (0, 2));
        assert_eq!(edit.range.end_ms, edit.duration_ms);
    }

    #[test]
    fn a_whole_range_grows_with_the_frames_that_decode() {
        let mut edit = strip(&[10, 20]);
        let more = strip(&[30, 40]).frames;
        edit.push_frames(more.clone());
        assert_eq!(edit.frame_range, (0, 3));
        assert_eq!(edit.range.end_ms, 100);
        edit.set_end(15);
        edit.push_frames(more);
        assert_eq!(edit.frame_range, (0, 1));
        assert_eq!(edit.duration_ms, 170);
    }

    #[test]
    fn a_text_end_steps_from_gap_to_gap_and_keeps_one_frame() {
        let mut edit = strip(&[100, 50, 250, 30]);
        let index = edit.add_text();
        edit.drag_text_edge(index, Edge::Start, 130);
        assert_eq!(edit.texts[index].start_ms, 150);
        edit.drag_text_edge(index, Edge::End, 160);
        assert_eq!(edit.texts[index].end_ms, 400);
        edit.drag_text_edge(index, Edge::End, 0);
        assert_eq!(edit.texts[index].end_ms, 400);
        edit.drag_text_edge(index, Edge::End, 390);
        assert_eq!(edit.texts[index].end_ms, 400);
        edit.drag_text_edge(index, Edge::Start, 9_000);
        assert_eq!(edit.texts[index].start_ms, 150);
        // A skipped frame has no time, so its gaps are one.
        edit.toggle_skip(&BTreeSet::from([1]));
        edit.drag_text_edge(index, Edge::Start, 90);
        assert_eq!(edit.texts[index].start_ms, 100);
    }

    #[test]
    fn a_moved_text_starts_at_a_gap_and_keeps_its_length() {
        let mut edit = strip(&[100, 50, 250, 30]);
        let index = edit.add_text();
        edit.drag_text_edge(index, Edge::End, 150);
        edit.move_text(index, (0, 150), 130);
        assert_eq!(
            (edit.texts[index].start_ms, edit.texts[index].end_ms),
            (150, 300)
        );
        edit.move_text(index, (0, 150), 9_000);
        assert_eq!(
            (edit.texts[index].start_ms, edit.texts[index].end_ms),
            (150, 300)
        );
    }

    #[test]
    fn a_short_delay_plays_as_a_browser_plays_it() {
        assert_eq!(played(0), 100);
        assert_eq!(played(10), 100);
        assert_eq!(played(20), 20);
    }
}
