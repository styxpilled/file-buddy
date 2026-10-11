//! An image sequence: stills as the frames of an animation, on a canvas. Each
//! image fits or fills the canvas, and then has its own offset and scale. The
//! frames keep their images as they are, and the export composes each one on
//! the canvas.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use image::imageops::{self, FilterType};

use super::{Edit, Frame, MIN_CROP};
use crate::ffmpeg::{self, Tool};

/// The delay of each image when the sequence opens.
pub const DELAY_MS: u32 = 100;
/// The scale of a frame stays in this range, so a frame cannot vanish or grow
/// past what the export can compose.
const MIN_SCALE: f64 = 0.05;
const MAX_SCALE: f64 = 20.;
/// The largest canvas side the panel takes.
pub const MAX_CANVAS: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// The whole image shows, with transparent bars.
    Fit,
    /// The image covers the canvas, centered.
    Fill,
}

/// A frame's own move and scale, on top of the fit or fill. The offset is a
/// fraction of the canvas, so a new canvas size keeps the frame in place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

impl Default for Place {
    fn default() -> Self {
        Self {
            x: 0.,
            y: 0.,
            scale: 1.,
        }
    }
}

/// Where an image goes on the canvas, in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub fn rect(image: (u32, u32), canvas: (u32, u32), fit: Fit, place: Place) -> Rect {
    let (image_width, image_height) = (f64::from(image.0.max(1)), f64::from(image.1.max(1)));
    let (canvas_width, canvas_height) = (f64::from(canvas.0), f64::from(canvas.1));
    let (across, down) = (canvas_width / image_width, canvas_height / image_height);
    let base = match fit {
        Fit::Fit => across.min(down),
        Fit::Fill => across.max(down),
    };
    let scale = base * place.scale;
    let (width, height) = (image_width * scale, image_height * scale);
    Rect {
        x: (canvas_width - width) / 2. + place.x * canvas_width,
        y: (canvas_height - height) / 2. + place.y * canvas_height,
        width,
        height,
    }
}

/// The image on a transparent canvas at `rect`. Only the part of the image on
/// the canvas is scaled, so a large scale costs no more than the canvas.
pub fn compose(image: &RgbaImage, canvas: (u32, u32), rect: Rect) -> RgbaImage {
    let mut out = RgbaImage::new(canvas.0, canvas.1);
    let (width, height) = image.dimensions();
    let scale = rect.width / f64::from(width.max(1));
    if scale <= 0. {
        return out;
    }
    let first = |at: f64| (-at / scale).floor().max(0.) as u32;
    let end = |at: f64, room: u32, size: u32| {
        ((f64::from(room) - at) / scale)
            .ceil()
            .clamp(0., f64::from(size)) as u32
    };
    let (left, top) = (first(rect.x).min(width), first(rect.y).min(height));
    let (right, bottom) = (end(rect.x, canvas.0, width), end(rect.y, canvas.1, height));
    if right <= left || bottom <= top {
        return out;
    }
    let part = imageops::crop_imm(image, left, top, right - left, bottom - top).to_image();
    let x = rect.x + f64::from(left) * scale;
    let y = rect.y + f64::from(top) * scale;
    let out_width = ((x + f64::from(right - left) * scale).round() - x.round()).max(1.);
    let out_height = ((y + f64::from(bottom - top) * scale).round() - y.round()).max(1.);
    let scaled = imageops::resize(
        &part,
        out_width as u32,
        out_height as u32,
        FilterType::Lanczos3,
    );
    imageops::overlay(&mut out, &scaled, x.round() as i64, y.round() as i64);
    out
}

/// The stills in name order, as `sort_name` orders a folder.
pub fn name_order(paths: &mut [PathBuf]) {
    paths.sort_by_cached_key(|path| {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        (name, path.clone())
    });
}

/// One still, decoded by FFmpeg as the still export reads it.
pub fn decode(path: &Path) -> Result<RgbaImage> {
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .arg("-i")
        .arg(path)
        .args(["-map", "0:v:0", "-frames:v", "1", "-c:v", "png"])
        .args(["-f", "image2pipe", "pipe:1"]);
    let png = ffmpeg::output(command).with_context(|| format!("decode {}", path.display()))?;
    if png.is_empty() {
        bail!("FFmpeg gave no image for {}", path.display());
    }
    Ok(
        image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .context("read the decoded image")?
            .to_rgba8(),
    )
}

/// The stills in order, one at a time, as they decode. A still that fails is
/// left out, and the error names the first that failed. `each` returns false to
/// stop.
pub fn decode_each(paths: &[PathBuf], mut each: impl FnMut(Frame) -> bool) -> Result<()> {
    let mut failed = Vec::new();
    for path in paths {
        match decode(path) {
            Ok(image) => {
                if !each(Frame::new(image, DELAY_MS)) {
                    return Ok(());
                }
            }
            Err(e) => failed.push(e),
        }
    }
    match failed.into_iter().next() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

impl Edit {
    /// Stills as frames, each at `DELAY_MS`. The canvas is the size of the
    /// first, and every image fits it.
    pub fn sequence(first: PathBuf, frames: Vec<Frame>) -> Self {
        Self {
            sequence: true,
            ..Self::with_frames(first, frames)
        }
    }

    pub fn is_sequence(&self) -> bool {
        self.sequence
    }

    /// Where the frame goes on the canvas. `None` outside frame mode.
    pub fn placed(&self, frame: &Frame) -> Option<Rect> {
        self.framed()
            .then(|| rect(frame.image.dimensions(), self.size, self.fit, frame.place))
    }

    /// The pixels the export takes for a frame: on the canvas in frame mode,
    /// and as it is otherwise. A frame that fills the canvas as it is needs no
    /// work.
    pub fn frame_pixels(&self, frame: &Frame) -> Arc<RgbaImage> {
        if frame.place == Place::default() && frame.image.dimensions() == self.size {
            return frame.image.clone();
        }
        match self.placed(frame) {
            Some(rect) => Arc::new(compose(&frame.image, self.size, rect)),
            None => frame.image.clone(),
        }
    }

    /// A new canvas, no smaller than the smallest crop. The crop goes back to
    /// the whole of it.
    pub fn set_canvas(&mut self, size: (u32, u32)) {
        let side = |pixels: u32| pixels.clamp(MIN_CROP, MAX_CANVAS);
        let size = (side(size.0), side(size.1));
        if size != self.size {
            self.size = size;
            self.reset_crop();
        }
    }

    /// Each frame by `dx`, `dy` in canvas pixels from where it was when the
    /// drag started. An edge of the image within `snap` canvas pixels of an
    /// edge of the canvas goes onto it.
    pub fn move_placed(&mut self, starts: &[(usize, Place)], dx: f64, dy: f64, snap: f64) {
        let (fit, size) = (self.fit, self.size);
        let (width, height) = (f64::from(size.0.max(1)), f64::from(size.1.max(1)));
        for &(index, start) in starts {
            let Some(frame) = self.frames.get_mut(index) else {
                continue;
            };
            let mut place = Place {
                x: start.x + dx / width,
                y: start.y + dy / height,
                ..start
            };
            let at = rect(frame.image.dimensions(), size, fit, place);
            place.x += snap_shift(at.x, at.width, width, snap) / width;
            place.y += snap_shift(at.y, at.height, height, snap) / height;
            frame.place = place;
        }
    }

    /// The wheel. Each frame grows or shrinks about its own center.
    pub fn scale_placed(&mut self, selected: &BTreeSet<usize>, factor: f64) {
        for &index in selected {
            if let Some(frame) = self.frames.get_mut(index) {
                frame.place.scale = (frame.place.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
            }
        }
    }

    /// Reset. The frames go back to the fit or the fill.
    pub fn reset_placed(&mut self, selected: &BTreeSet<usize>) {
        for &index in selected {
            if let Some(frame) = self.frames.get_mut(index) {
                frame.place = Place::default();
            }
        }
    }

    /// The frame that shows under the frame at `index` while it is selected:
    /// the one before it that plays.
    pub fn frame_before(&self, index: usize) -> Option<usize> {
        self.frames
            .get(..index)?
            .iter()
            .rposition(|frame| !frame.skipped)
    }
}

/// The shift that puts the nearer side of a span from `start` and `length`
/// onto a side of `0..room`, if it is within `snap`.
fn snap_shift(start: f64, length: f64, room: f64, snap: f64) -> f64 {
    let end = start + length;
    [-start, room - end]
        .into_iter()
        .filter(|shift| shift.abs() <= snap)
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or(0.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Crop;

    const WIDE: (u32, u32) = (400, 100);
    const TALL: (u32, u32) = (100, 400);
    const CANVAS: (u32, u32) = (200, 200);

    fn at(rect: Rect) -> (f64, f64, f64, f64) {
        (rect.x, rect.y, rect.width, rect.height)
    }

    #[test]
    fn fit_shows_the_whole_image_and_fill_covers_the_canvas() {
        let place = Place::default();
        assert_eq!(
            at(rect(WIDE, CANVAS, Fit::Fit, place)),
            (0., 75., 200., 50.)
        );
        assert_eq!(
            at(rect(TALL, CANVAS, Fit::Fit, place)),
            (75., 0., 50., 200.)
        );
        assert_eq!(
            at(rect(WIDE, CANVAS, Fit::Fill, place)),
            (-300., 0., 800., 200.)
        );
        assert_eq!(
            at(rect(TALL, CANVAS, Fit::Fill, place)),
            (0., -300., 200., 800.)
        );
    }

    #[test]
    fn the_offset_is_a_share_of_the_canvas_and_the_scale_keeps_the_center() {
        let place = Place {
            x: 0.25,
            y: -0.1,
            scale: 2.,
        };
        // Fit makes the wide image 200 x 50, the scale 400 x 100, centered
        // at 100, 100, and then 50 right and 20 up.
        assert_eq!(
            at(rect(WIDE, CANVAS, Fit::Fit, place)),
            (-50., 30., 400., 100.)
        );
        assert_eq!(
            at(rect(TALL, CANVAS, Fit::Fit, place)),
            (100., -120., 100., 400.)
        );
        // Fill makes the tall image 200 x 800, and the scale 400 x 1600.
        assert_eq!(
            at(rect(TALL, CANVAS, Fit::Fill, place)),
            (-50., -720., 400., 1600.)
        );
        // On a wide canvas the same place moves twice as far across.
        assert_eq!(
            at(rect(WIDE, (400, 200), Fit::Fill, place)),
            (-500., -120., 1600., 400.)
        );
    }

    /// An image of `size` in one color, with a red column at its left side.
    fn marked(size: (u32, u32)) -> RgbaImage {
        RgbaImage::from_fn(size.0, size.1, |x, _| {
            if x == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        })
    }

    #[test]
    fn fit_leaves_transparent_bars_and_fill_has_none() {
        let fitted = compose(
            &marked(WIDE),
            CANVAS,
            rect(WIDE, CANVAS, Fit::Fit, Place::default()),
        );
        assert_eq!(fitted.dimensions(), CANVAS);
        assert_eq!(fitted.get_pixel(100, 10)[3], 0);
        assert_eq!(fitted.get_pixel(100, 100).0, [0, 0, 255, 255]);
        assert_eq!(fitted.get_pixel(100, 190)[3], 0);
        let filled = compose(
            &marked(TALL),
            CANVAS,
            rect(TALL, CANVAS, Fit::Fill, Place::default()),
        );
        assert!(filled.pixels().all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn a_composed_frame_has_its_offset_and_scale() {
        // The wide image fits at 200 x 50. At scale 2 it is 400 x 100, and an
        // offset of three quarters puts its left side, the red column, at 50.
        let place = Place {
            x: 0.75,
            y: 0.,
            scale: 2.,
        };
        let frame = compose(&marked(WIDE), CANVAS, rect(WIDE, CANVAS, Fit::Fit, place));
        assert_eq!(frame.get_pixel(49, 100)[3], 0);
        assert_eq!(frame.get_pixel(50, 100).0, [255, 0, 0, 255]);
        assert_eq!(frame.get_pixel(60, 100).0, [0, 0, 255, 255]);
        assert_eq!(frame.get_pixel(100, 49)[3], 0);
        assert_eq!(frame.get_pixel(100, 50)[3], 255);
        assert_eq!(frame.get_pixel(100, 149)[3], 255);
        assert_eq!(frame.get_pixel(100, 150)[3], 0);
    }

    #[test]
    fn a_large_scale_composes_only_what_shows() {
        let place = Place {
            x: 0.,
            y: 0.,
            scale: MAX_SCALE,
        };
        let image = marked((4000, 3000));
        let frame = compose(&image, CANVAS, rect((4000, 3000), CANVAS, Fit::Fill, place));
        assert!(frame.pixels().all(|pixel| pixel.0 == [0, 0, 255, 255]));
        let gone = Place {
            x: 5.,
            ..Place::default()
        };
        let frame = compose(&image, CANVAS, rect((4000, 3000), CANVAS, Fit::Fit, gone));
        assert!(frame.pixels().all(|pixel| pixel[3] == 0));
    }

    #[test]
    fn a_frame_moves_from_its_start_and_scales_in_bounds() {
        let frames = vec![
            Frame::new(marked(WIDE), DELAY_MS),
            Frame::new(marked(TALL), DELAY_MS),
        ];
        let mut edit = Edit::sequence("a.png".into(), frames);
        assert_eq!(edit.size, WIDE);
        assert_eq!(edit.range.end_ms, 2 * u64::from(DELAY_MS));
        let start = edit.frames[1].place;
        edit.move_placed(&[(1, start)], 40., -10., 0.);
        assert_eq!(
            edit.frames[1].place,
            Place {
                x: 0.1,
                y: -0.1,
                scale: 1.
            }
        );
        let one = BTreeSet::from([1]);
        edit.scale_placed(&one, 1000.);
        assert_eq!(edit.frames[1].place.scale, MAX_SCALE);
        edit.reset_placed(&one);
        assert_eq!(edit.frames[1].place, Place::default());
        assert_eq!(edit.frames[0].place, Place::default());
    }

    #[test]
    fn an_edge_near_a_canvas_edge_snaps_to_it() {
        let frames = vec![
            Frame::new(marked(WIDE), DELAY_MS),
            Frame::new(marked(TALL), DELAY_MS),
        ];
        let mut edit = Edit::sequence("a.png".into(), frames);
        edit.set_canvas(CANVAS);
        // The tall image fits at 50 x 200 from 75. Its left side moves to 3,
        // and snaps to 0. Its top is on the canvas top already.
        edit.move_placed(&[(1, Place::default())], -72., 4., 5.);
        let at = edit.placed(&edit.frames[1]).unwrap();
        assert!(at.x.abs() < 1e-9 && at.y.abs() < 1e-9, "{at:?}");
        // Its right side moves to 194, and snaps to 200.
        edit.move_placed(&[(1, Place::default())], 69., 0., 8.);
        assert!((edit.placed(&edit.frames[1]).unwrap().x - 150.).abs() < 1e-9);
        // Too far to snap.
        edit.move_placed(&[(1, Place::default())], 60., 0., 8.);
        assert!((edit.placed(&edit.frames[1]).unwrap().x - 135.).abs() < 1e-9);
    }

    #[test]
    fn a_new_canvas_resets_the_crop_and_has_bounds() {
        let frames = vec![Frame::new(marked(WIDE), DELAY_MS)];
        let mut edit = Edit::sequence("a.png".into(), frames);
        edit.crop.width = 100;
        edit.set_canvas((300, 0));
        assert_eq!(edit.size, (300, MIN_CROP));
        assert_eq!(edit.crop, Crop::full((300, MIN_CROP)));
        edit.set_canvas((99_999, 500));
        assert_eq!(edit.size, (MAX_CANVAS, 500));
    }

    #[test]
    fn the_frame_under_is_the_one_before_that_plays() {
        let frames = (0..4)
            .map(|_| Frame::new(marked((2, 2)), DELAY_MS))
            .collect();
        let mut edit = Edit::sequence("a.png".into(), frames);
        assert_eq!(edit.frame_before(0), None);
        assert_eq!(edit.frame_before(2), Some(1));
        edit.toggle_skip(&BTreeSet::from([1]));
        assert_eq!(edit.frame_before(2), Some(0));
    }

    #[test]
    fn stills_go_in_name_order() {
        let mut paths: Vec<PathBuf> = ["D:/b/c.png", "D:/a/B.png", "D:/c/a.png"]
            .map(PathBuf::from)
            .into();
        name_order(&mut paths);
        assert_eq!(
            paths,
            ["D:/c/a.png", "D:/a/B.png", "D:/b/c.png"].map(PathBuf::from)
        );
    }
}
