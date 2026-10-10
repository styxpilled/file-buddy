//! Frame mode: every frame of a GIF or an animated WebP in memory, each with
//! its delay, and the edits of the strip. Text times are in output time, so a
//! text stays at its moment when frames go or move.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, RgbaImage};
use image_webp::WebPDecoder;

use super::{Edit, Range};

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
}

impl PartialEq for Frame {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.image, &other.image) && self.delay_ms == other.delay_ms
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("size", &self.image.dimensions())
            .field("delay_ms", &self.delay_ms)
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
    let mut head = [0u8; 12];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .with_context(|| format!("read {}", path.display()))?;
    let frames = if head.starts_with(b"GIF8") {
        gif(path)?
    } else if &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        webp(path)?
    } else {
        bail!("not a GIF or a WebP");
    };
    if frames.is_empty() {
        bail!("the animation has no frames");
    }
    Ok(frames)
}

fn gif(path: &Path) -> Result<Vec<Frame>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let decoder = GifDecoder::new(BufReader::new(file)).context("decode the GIF")?;
    decoder
        .into_frames()
        .map(|frame| {
            let frame = frame.context("decode a frame")?;
            let (numerator, denominator) = frame.delay().numer_denom_ms();
            Ok(Frame {
                delay_ms: played(numerator / denominator.max(1)),
                image: Arc::new(frame.into_buffer()),
            })
        })
        .collect()
}

fn webp(path: &Path) -> Result<Vec<Frame>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut decoder = WebPDecoder::new(BufReader::new(file)).context("decode the WebP")?;
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
        return Ok(vec![Frame {
            image: Arc::new(rgba(&buffer)?),
            delay_ms: SHORT_DELAY_PLAYS_AT,
        }]);
    }
    (0..decoder.num_frames())
        .map(|_| {
            let delay = decoder.read_frame(&mut buffer).context("decode a frame")?;
            Ok(Frame {
                image: Arc::new(rgba(&buffer)?),
                delay_ms: played(delay),
            })
        })
        .collect()
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
            ..Self::new(source.clone(), source, 0, size)
        };
        edit.retime();
        edit
    }

    pub fn framed(&self) -> bool {
        !self.frames.is_empty()
    }

    /// The length and the range follow the strip. Frame mode has no range of
    /// its own.
    fn retime(&mut self) {
        let total = self.frames.iter().map(|f| u64::from(f.delay_ms)).sum();
        self.duration_ms = total;
        self.range = Range {
            start_ms: 0,
            end_ms: total,
        };
    }

    pub fn frame_start_ms(&self, index: usize) -> u64 {
        self.frames[..index.min(self.frames.len())]
            .iter()
            .map(|f| u64::from(f.delay_ms))
            .sum()
    }

    /// The frame that shows at `at_ms` of the strip. The last one holds past
    /// the end.
    pub fn frame_at(&self, at_ms: u64) -> usize {
        let mut end = 0;
        for (index, frame) in self.frames.iter().enumerate() {
            end += u64::from(frame.delay_ms);
            if at_ms < end {
                return index;
            }
        }
        self.frames.len().saturating_sub(1)
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
        let mut index = 0;
        self.frames.retain(|_| {
            let keep = !selected.contains(&index);
            index += 1;
            keep
        });
        self.retime();
        Some(first.min(self.frames.len() - 1))
    }

    /// `ctrl-d`. The copies go after the last selected frame, in order, and
    /// become the selection.
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
        self.frames.splice(at..at, copies);
        self.retime();
        (at..at + count).collect()
    }

    /// A drag puts the selected frames in the gap before `gap`, from 0 to the
    /// length of the strip. They keep their order and stay selected.
    pub fn move_frames(&mut self, selected: &BTreeSet<usize>, gap: usize) -> BTreeSet<usize> {
        let gap = gap.min(self.frames.len());
        let (mut moved, mut rest) = (Vec::new(), Vec::new());
        for (index, frame) in self.frames.drain(..).enumerate() {
            if selected.contains(&index) {
                moved.push(frame);
            } else {
                rest.push(frame);
            }
        }
        let at = gap - selected.range(..gap).count();
        let count = moved.len();
        rest.splice(at..at, moved);
        self.frames = rest;
        (at..at + count).collect()
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
            .map(|(at, &delay_ms)| Frame {
                image: Arc::new(RgbaImage::from_pixel(
                    1,
                    1,
                    image::Rgba([at as u8, 0, 0, 255]),
                )),
                delay_ms,
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

    #[test]
    fn a_short_delay_plays_as_a_browser_plays_it() {
        assert_eq!(played(0), 100);
        assert_eq!(played(10), 100);
        assert_eq!(played(20), 20);
    }
}
