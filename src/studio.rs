//! The edit the Studio holds: a source, the range cut from it, the crop and the
//! scale. Nothing of it goes to the database. `ffmpeg::export` makes a file from
//! it.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// The file as the index knows it. The export takes its name from it.
    pub source: PathBuf,
    /// What FFmpeg reads: the file, or its proxy.
    pub input: PathBuf,
    pub duration_ms: u64,
    /// The frame size of the input, in pixels. Zero when it is not known, and
    /// then the crop and the scale do not apply.
    pub size: (u32, u32),
    pub range: Range,
    pub crop: Crop,
    pub aspect: Aspect,
    pub max_width: MaxWidth,
}

impl Edit {
    /// The whole source, at its own size.
    pub fn new(source: PathBuf, input: PathBuf, duration_ms: u64, size: (u32, u32)) -> Self {
        Self {
            source,
            input,
            duration_ms,
            size,
            range: Range {
                start_ms: 0,
                end_ms: duration_ms,
            },
            crop: Crop::full(size),
            aspect: Aspect::Free,
            max_width: MaxWidth::Source,
        }
    }

    /// The crop shrinks to the largest box of the aspect inside it, at its
    /// center.
    pub fn set_aspect(&mut self, aspect: Aspect) {
        self.aspect = aspect;
        if let Some(ratio) = aspect.ratio(self.size) {
            self.crop = self.crop.fit(ratio);
        }
    }

    /// A drag on the crop box, `dx` and `dy` source pixels from where it
    /// started. `start` is the crop when the drag started.
    pub fn drag_crop(&mut self, grip: Grip, start: Crop, dx: f64, dy: f64) {
        self.crop = start.dragged(grip, dx, dy, self.aspect.ratio(self.size), self.size);
    }

    pub fn reset_crop(&mut self) {
        self.crop = Crop::full(self.size);
        self.aspect = Aspect::Free;
    }

    /// The size of the exported frame: the crop, even, and no wider than the
    /// maximum width. It never grows past the crop.
    pub fn output_size(&self) -> (u32, u32) {
        let (width, height) = (even(self.crop.width), even(self.crop.height));
        let limit = match self.max_width {
            MaxWidth::Source => width,
            MaxWidth::Pixels(max) => even(max).min(width),
        };
        if limit >= width {
            return (width, height);
        }
        let scaled = (f64::from(height) * f64::from(limit) / f64::from(width)).round() as u32;
        (limit, even(scaled))
    }

    /// `[`. An in point at or past the out point moves the out point to the end.
    pub fn set_start(&mut self, at_ms: u64) {
        self.range.start_ms = at_ms.min(self.duration_ms);
        if self.range.end_ms <= self.range.start_ms {
            self.range.end_ms = self.duration_ms;
        }
    }

    /// `]`. An out point at or before the in point moves the in point to the
    /// start.
    pub fn set_end(&mut self, at_ms: u64) {
        self.range.end_ms = at_ms.min(self.duration_ms);
        if self.range.end_ms <= self.range.start_ms {
            self.range.start_ms = 0;
        }
    }

    /// A drag on a handle. It stops at the other handle.
    pub fn drag(&mut self, edge: Edge, at_ms: u64) {
        match edge {
            Edge::Start => self.range.start_ms = at_ms.min(self.range.end_ms),
            Edge::End => {
                self.range.end_ms = at_ms.clamp(self.range.start_ms, self.duration_ms);
            }
        }
    }
}

/// H.264 needs even dimensions.
fn even(pixels: u32) -> u32 {
    (pixels & !1).max(2)
}

/// The smallest crop, in source pixels.
pub const MIN_CROP: u32 = 16;

/// A box in source pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Crop {
    pub fn full((width, height): (u32, u32)) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    /// The largest box of this ratio inside the crop, at its center.
    fn fit(self, ratio: f64) -> Self {
        let (width, height) = (f64::from(self.width), f64::from(self.height));
        let (fitted_width, fitted_height) = if width / height > ratio {
            (height * ratio, height)
        } else {
            (width, width / ratio)
        };
        let fitted_width = fitted_width.round() as u32;
        let fitted_height = fitted_height.round() as u32;
        Self {
            x: self.x + (self.width - fitted_width) / 2,
            y: self.y + (self.height - fitted_height) / 2,
            width: fitted_width,
            height: fitted_height,
        }
    }

    /// The crop after a drag of `dx`, `dy` pixels on a grip. It stays inside
    /// the frame. With a ratio, the side or corner across from the grip stays
    /// where it is, and a grip on one edge keeps the box centered on the
    /// other axis.
    fn dragged(self, grip: Grip, dx: f64, dy: f64, ratio: Option<f64>, size: (u32, u32)) -> Self {
        let (frame_width, frame_height) = (f64::from(size.0), f64::from(size.1));
        let min = f64::from(MIN_CROP);
        let mut left = f64::from(self.x);
        let mut top = f64::from(self.y);
        let mut right = left + f64::from(self.width);
        let mut bottom = top + f64::from(self.height);
        let (horizontal, vertical) = grip.sides();

        if grip == Grip::Move {
            let x = (left + dx).clamp(0., frame_width - f64::from(self.width));
            let y = (top + dy).clamp(0., frame_height - f64::from(self.height));
            return Self {
                x: x.round() as u32,
                y: y.round() as u32,
                ..self
            };
        }
        match horizontal {
            Side::Start => left = (left + dx).clamp(0., right - min),
            Side::End => right = (right + dx).clamp(left + min, frame_width),
            Side::None => {}
        }
        match vertical {
            Side::Start => top = (top + dy).clamp(0., bottom - min),
            Side::End => bottom = (bottom + dy).clamp(top + min, frame_height),
            Side::None => {}
        }

        if let Some(ratio) = ratio {
            let center_x = f64::from(self.x) + f64::from(self.width) / 2.;
            let center_y = f64::from(self.y) + f64::from(self.height) / 2.;
            let room_width = match horizontal {
                Side::Start => right,
                Side::End => frame_width - left,
                Side::None => 2. * center_x.min(frame_width - center_x),
            };
            let room_height = match vertical {
                Side::Start => bottom,
                Side::End => frame_height - top,
                Side::None => 2. * center_y.min(frame_height - center_y),
            };
            let wanted = match horizontal {
                Side::None => (bottom - top) * ratio,
                _ => right - left,
            };
            let width = wanted
                .max(min.max(min * ratio))
                .min(room_width)
                .min(room_height * ratio);
            let height = width / ratio;
            (left, right) = match horizontal {
                Side::Start => (right - width, right),
                Side::End => (left, left + width),
                Side::None => (center_x - width / 2., center_x + width / 2.),
            };
            (top, bottom) = match vertical {
                Side::Start => (bottom - height, bottom),
                Side::End => (top, top + height),
                Side::None => (center_y - height / 2., center_y + height / 2.),
            };
        }

        let x = left.round().max(0.) as u32;
        let y = top.round().max(0.) as u32;
        Self {
            x,
            y,
            width: ((right - left).round() as u32).min(size.0 - x),
            height: ((bottom - top).round() as u32).min(size.1 - y),
        }
    }
}

/// The part of the crop box a drag holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grip {
    Move,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// The side of the box a grip moves, on one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    None,
    Start,
    End,
}

impl Grip {
    pub const EDGES: [Grip; 8] = [
        Grip::Left,
        Grip::Right,
        Grip::Top,
        Grip::Bottom,
        Grip::TopLeft,
        Grip::TopRight,
        Grip::BottomLeft,
        Grip::BottomRight,
    ];

    /// The sides the grip moves: horizontal, then vertical.
    fn sides(self) -> (Side, Side) {
        match self {
            Grip::Move => (Side::None, Side::None),
            Grip::Left => (Side::Start, Side::None),
            Grip::Right => (Side::End, Side::None),
            Grip::Top => (Side::None, Side::Start),
            Grip::Bottom => (Side::None, Side::End),
            Grip::TopLeft => (Side::Start, Side::Start),
            Grip::TopRight => (Side::End, Side::Start),
            Grip::BottomLeft => (Side::Start, Side::End),
            Grip::BottomRight => (Side::End, Side::End),
        }
    }

    /// Where the grip sits on the box, from 0 to 1 on each axis.
    pub fn anchor(self) -> (f32, f32) {
        let at = |side| match side {
            Side::Start => 0.,
            Side::None => 0.5,
            Side::End => 1.,
        };
        let (horizontal, vertical) = self.sides();
        (at(horizontal), at(vertical))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aspect {
    Free,
    Source,
    Square,
    Wide,
    Tall,
}

impl Aspect {
    pub const ALL: [Aspect; 5] = [
        Aspect::Free,
        Aspect::Source,
        Aspect::Square,
        Aspect::Wide,
        Aspect::Tall,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Aspect::Free => "Free",
            Aspect::Source => "Source",
            Aspect::Square => "1:1",
            Aspect::Wide => "16:9",
            Aspect::Tall => "9:16",
        }
    }

    /// Width over height. Free has none.
    pub fn ratio(self, (width, height): (u32, u32)) -> Option<f64> {
        match self {
            Aspect::Free => None,
            Aspect::Source if width == 0 || height == 0 => None,
            Aspect::Source => Some(f64::from(width) / f64::from(height)),
            Aspect::Square => Some(1.),
            Aspect::Wide => Some(16. / 9.),
            Aspect::Tall => Some(9. / 16.),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxWidth {
    Source,
    Pixels(u32),
}

/// The maximum widths the panel offers, after Source.
pub const MAX_WIDTHS: [u32; 4] = [1080, 720, 480, 360];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Range {
    pub fn len_ms(self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    pub fn at(self, edge: Edge) -> u64 {
        match edge {
            Edge::Start => self.start_ms,
            Edge::End => self.end_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Best,
    High,
    Medium,
    Low,
    Lower,
    Lowest,
}

impl Quality {
    pub const ALL: [Quality; 6] = [
        Quality::Best,
        Quality::High,
        Quality::Medium,
        Quality::Low,
        Quality::Lower,
        Quality::Lowest,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Best => "Best",
            Quality::High => "High",
            Quality::Medium => "Medium",
            Quality::Low => "Low",
            Quality::Lower => "Lower",
            Quality::Lowest => "Lowest",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Mp4,
    Webm,
    Gif,
    Webp,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Mp4, Format::Webm, Format::Gif, Format::Webp];

    pub fn label(self) -> &'static str {
        match self {
            Format::Mp4 => "MP4",
            Format::Webm => "WebM",
            Format::Gif => "GIF",
            Format::Webp => "WebP",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Mp4 => "mp4",
            Format::Webm => "webm",
            Format::Gif => "gif",
            Format::Webp => "webp",
        }
    }

    /// The smallest even frame the encoder takes. NVENC refuses smaller ones:
    /// `h264_nvenc` takes 145 x 49 and up, and `av1_nvenc` 129 x 65.
    pub fn min_size(self) -> (u32, u32) {
        match self {
            Format::Mp4 => (146, 50),
            Format::Webm => (130, 66),
            Format::Gif | Format::Webp => (2, 2),
        }
    }

    pub fn has_quality(self) -> bool {
        self != Format::Gif
    }

    pub fn has_fps(self) -> bool {
        matches!(self, Format::Gif | Format::Webp)
    }
}

/// The frame rates a GIF or a WebP export can take.
pub const FPS: [u32; 5] = [10, 15, 20, 25, 30];

/// The export controls. Each format keeps its own until the application
/// closes, so they live apart from the edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Controls {
    pub format: Format,
    pub mp4_quality: Quality,
    pub webm_quality: Quality,
    pub gif_fps: u32,
    pub webp_fps: u32,
    pub webp_quality: Quality,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            format: Format::Mp4,
            mp4_quality: Quality::Low,
            webm_quality: Quality::Low,
            gif_fps: 15,
            webp_fps: 20,
            webp_quality: Quality::Low,
        }
    }
}

impl Controls {
    /// The quality of the chosen format. GIF has none.
    pub fn quality(&self) -> Option<Quality> {
        match self.format {
            Format::Mp4 => Some(self.mp4_quality),
            Format::Webm => Some(self.webm_quality),
            Format::Gif => None,
            Format::Webp => Some(self.webp_quality),
        }
    }

    pub fn set_quality(&mut self, quality: Quality) {
        match self.format {
            Format::Mp4 => self.mp4_quality = quality,
            Format::Webm => self.webm_quality = quality,
            Format::Gif => {}
            Format::Webp => self.webp_quality = quality,
        }
    }

    /// The frame rate of the chosen format. MP4 and WebM keep the source rate.
    pub fn fps(&self) -> Option<u32> {
        match self.format {
            Format::Mp4 | Format::Webm => None,
            Format::Gif => Some(self.gif_fps),
            Format::Webp => Some(self.webp_fps),
        }
    }

    pub fn set_fps(&mut self, fps: u32) {
        match self.format {
            Format::Mp4 | Format::Webm => {}
            Format::Gif => self.gif_fps = fps,
            Format::Webp => self.webp_fps = fps,
        }
    }
}

/// `<source stem> <in>-<out>.<ext>`, for example `cat jump 0m03-0m07.mp4`.
pub fn export_name(source: &Path, range: Range, format: Format) -> String {
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!(
        "{stem} {}-{}.{}",
        stamp(range.start_ms),
        stamp(range.end_ms),
        format.extension()
    )
}

/// `0m03`, or `1h02m03` from an hour.
fn stamp(ms: u64) -> String {
    let total = ms / 1000;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}")
    } else {
        format!("{minutes}m{seconds:02}")
    }
}

/// `name` in `folder`, or with ` (2)`, ` (3)` and so on if that name exists.
pub fn unique_path(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (name, String::new()),
    };
    (2..)
        .map(|n| folder.join(format!("{stem} ({n}){extension}")))
        .find(|path| !path.exists())
        .expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit() -> Edit {
        Edit::new("a.mp4".into(), "a.mp4".into(), 10_000, (1920, 1080))
    }

    fn crop(x: u32, y: u32, width: u32, height: u32) -> Crop {
        Crop {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn an_aspect_takes_the_largest_box_at_the_center() {
        let mut edit = edit();
        edit.set_aspect(Aspect::Square);
        assert_eq!(edit.crop, crop(420, 0, 1080, 1080));
        edit.set_aspect(Aspect::Tall);
        assert_eq!(edit.crop, crop(656, 0, 608, 1080));
        edit.set_aspect(Aspect::Free);
        assert_eq!(edit.crop, crop(656, 0, 608, 1080));
    }

    #[test]
    fn a_move_stays_inside_the_frame() {
        let mut edit = edit();
        let start = crop(100, 100, 400, 300);
        edit.drag_crop(Grip::Move, start, -500., 50.);
        assert_eq!(edit.crop, crop(0, 150, 400, 300));
        edit.drag_crop(Grip::Move, start, 5000., 5000.);
        assert_eq!(edit.crop, crop(1520, 780, 400, 300));
    }

    #[test]
    fn a_free_corner_moves_two_sides_and_stops_at_the_frame() {
        let mut edit = edit();
        let start = crop(100, 100, 400, 300);
        edit.drag_crop(Grip::BottomRight, start, 50., 2000.);
        assert_eq!(edit.crop, crop(100, 100, 450, 980));
        edit.drag_crop(Grip::TopLeft, start, 1000., 0.);
        assert_eq!(edit.crop, crop(100 + 400 - MIN_CROP, 100, MIN_CROP, 300));
    }

    #[test]
    fn a_locked_aspect_keeps_its_ratio_on_a_corner_and_an_edge() {
        let mut edit = edit();
        edit.aspect = Aspect::Square;
        let start = crop(100, 100, 400, 400);
        // The corner across from the grip stays.
        edit.drag_crop(Grip::TopLeft, start, -50., 0.);
        assert_eq!(edit.crop, crop(50, 50, 450, 450));
        // An edge keeps the box centered on the other axis.
        edit.drag_crop(Grip::Right, start, 100., 0.);
        assert_eq!(edit.crop, crop(100, 50, 500, 500));
        // The frame stops it, and the ratio holds.
        edit.drag_crop(Grip::BottomRight, start, 5000., 0.);
        assert_eq!(edit.crop, crop(100, 100, 980, 980));
    }

    #[test]
    fn the_output_is_even_and_never_grows_past_the_crop() {
        let mut edit = edit();
        edit.crop = crop(0, 0, 641, 359);
        assert_eq!(edit.output_size(), (640, 358));
        edit.max_width = MaxWidth::Pixels(480);
        assert_eq!(edit.output_size(), (480, 268));
        edit.max_width = MaxWidth::Pixels(1080);
        assert_eq!(edit.output_size(), (640, 358));
        edit.max_width = MaxWidth::Pixels(361);
        assert_eq!(edit.output_size(), (360, 200));
    }

    #[test]
    fn the_name_holds_the_stem_and_the_range() {
        let range = Range {
            start_ms: 3_400,
            end_ms: 7_900,
        };
        assert_eq!(
            export_name(Path::new("D:/clips/cat jump.mp4"), range, Format::Mp4),
            "cat jump 0m03-0m07.mp4"
        );
        let range = Range {
            start_ms: 62_000,
            end_ms: 3_723_000,
        };
        assert_eq!(
            export_name(Path::new("a.b.webm"), range, Format::Mp4),
            "a.b 1m02-1h02m03.mp4"
        );
    }

    #[test]
    fn a_name_that_exists_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let name = "cat jump 0m03-0m07.mp4";
        assert_eq!(unique_path(dir.path(), name), dir.path().join(name));
        std::fs::write(dir.path().join(name), b"").unwrap();
        let second = dir.path().join("cat jump 0m03-0m07 (2).mp4");
        assert_eq!(unique_path(dir.path(), name), second);
        std::fs::write(&second, b"").unwrap();
        assert_eq!(
            unique_path(dir.path(), name),
            dir.path().join("cat jump 0m03-0m07 (3).mp4")
        );
    }

    #[test]
    fn each_format_keeps_its_own_controls() {
        let mut controls = Controls::default();
        controls.set_quality(Quality::Low);
        controls.format = Format::Webp;
        controls.set_quality(Quality::Best);
        controls.set_fps(10);
        controls.format = Format::Gif;
        controls.set_fps(25);
        assert_eq!(controls.quality(), None);
        assert_eq!(controls.fps(), Some(25));
        controls.format = Format::Webp;
        assert_eq!(
            (controls.quality(), controls.fps()),
            (Some(Quality::Best), Some(10))
        );
        controls.format = Format::Mp4;
        assert_eq!(
            (controls.quality(), controls.fps()),
            (Some(Quality::Low), None)
        );
        controls.format = Format::Webm;
        assert_eq!(controls.quality(), Some(Quality::Low));
    }

    #[test]
    fn the_keys_push_the_other_point_out_of_the_way() {
        let mut edit = edit();
        edit.set_end(4_000);
        edit.set_start(6_000);
        assert_eq!((edit.range.start_ms, edit.range.end_ms), (6_000, 10_000));
        edit.set_end(2_000);
        assert_eq!((edit.range.start_ms, edit.range.end_ms), (0, 2_000));
        edit.set_end(20_000);
        assert_eq!(edit.range.end_ms, 10_000);
    }

    #[test]
    fn a_drag_stops_at_the_other_handle() {
        let mut edit = edit();
        edit.set_start(3_000);
        edit.set_end(5_000);
        edit.drag(Edge::Start, 8_000);
        assert_eq!(edit.range.start_ms, 5_000);
        edit.drag(Edge::End, 1_000);
        assert_eq!(edit.range.end_ms, 5_000);
        edit.drag(Edge::End, 12_000);
        assert_eq!(edit.range.end_ms, 10_000);
    }
}
