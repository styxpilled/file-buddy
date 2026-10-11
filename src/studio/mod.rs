//! The edit the Studio holds: a source, the range cut from it or its frames,
//! the crop, the scale, the speed and the texts. Nothing of it goes to the
//! database. `ffmpeg::export` makes a file from it.

pub mod frames;
pub mod sequence;

use std::path::{Path, PathBuf};

pub use frames::Frame;
pub use sequence::Fit;

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
    /// One of `SPEEDS`.
    pub speed: f64,
    pub texts: Vec<Text>,
    /// One frame: no range, no speed, and no time on a text.
    pub still: bool,
    /// Frame mode, for a GIF or an animated WebP. Empty for any other source.
    pub frames: Vec<Frame>,
    /// In frame mode, the first and the last frame of the range. `range` is
    /// their time on the strip.
    pub frame_range: (usize, usize),
    /// In frame mode, how each frame takes the canvas, which is `size`.
    pub fit: Fit,
    /// Frame mode from stills. The export takes its name from the first.
    pub sequence: bool,
    /// The stem the user gave the export, in place of the one from the source.
    pub name: Option<String>,
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
            speed: 1.,
            texts: Vec::new(),
            still: false,
            frames: Vec::new(),
            frame_range: (0, 0),
            fit: Fit::Fit,
            sequence: false,
            name: None,
        }
    }

    pub fn still(source: PathBuf, size: (u32, u32)) -> Self {
        Self {
            still: true,
            ..Self::new(source.clone(), source, 0, size)
        }
    }

    /// `n`. A text at the center, for the whole range. Returns its index.
    pub fn add_text(&mut self) -> usize {
        self.texts.push(Text {
            words: "Text".into(),
            x: 0.5,
            y: 0.5,
            size: DEFAULT_TEXT_SIZE,
            boxed: false,
            start_ms: self.range.start_ms,
            end_ms: self.range.end_ms,
        });
        self.texts.len() - 1
    }

    /// A drag on the end of a text's bar. It stops at the other end. In frame
    /// mode it steps from gap to gap between frames, and keeps one frame.
    pub fn drag_text_edge(&mut self, index: usize, edge: Edge, at_ms: u64) {
        if self.framed() {
            let Some(text) = self.texts.get(index) else {
                return;
            };
            let other = match edge {
                Edge::Start => text.end_ms,
                Edge::End => text.start_ms,
            };
            let Some(at_ms) = self.text_edge_gap(edge, at_ms, other) else {
                return;
            };
            let text = &mut self.texts[index];
            match edge {
                Edge::Start => text.start_ms = at_ms,
                Edge::End => text.end_ms = at_ms,
            }
            return;
        }
        let duration_ms = self.duration_ms;
        if let Some(text) = self.texts.get_mut(index) {
            match edge {
                Edge::Start => text.start_ms = at_ms.min(text.end_ms),
                Edge::End => text.end_ms = at_ms.clamp(text.start_ms, duration_ms),
            }
        }
    }

    /// A drag on the middle of a text's bar, `delta_ms` from where it was when
    /// the drag started. The length stays, and the text stays in the source.
    /// In frame mode it starts at the gap between frames nearest the drag.
    pub fn move_text(&mut self, index: usize, (start_ms, end_ms): (u64, u64), delta_ms: i64) {
        let length = end_ms - start_ms;
        let latest = self.duration_ms.saturating_sub(length);
        let mut at = (start_ms as i64 + delta_ms).clamp(0, latest as i64) as u64;
        if self.framed() {
            at = self.text_start_gap(at, latest);
        }
        if let Some(text) = self.texts.get_mut(index) {
            text.start_ms = at;
            text.end_ms = at + length;
        }
    }

    /// Removes a text. Returns the text to select after it: the next one, or
    /// the one before when none is next.
    pub fn remove_text(&mut self, index: usize) -> Option<usize> {
        if index >= self.texts.len() {
            return None;
        }
        self.texts.remove(index);
        match self.texts.len() {
            0 => None,
            len => Some(index.min(len - 1)),
        }
    }

    /// When a text shows, in seconds from the in point, before the speed
    /// change. `None` when it does not show inside the range.
    pub fn text_window(&self, text: &Text) -> Option<(f64, f64)> {
        let start = text.start_ms.max(self.range.start_ms);
        let end = text.end_ms.min(self.range.end_ms);
        let seconds = |ms: u64| (ms - self.range.start_ms) as f64 / 1000.;
        (end > start).then(|| (seconds(start), seconds(end)))
    }

    /// Whether a text shows at `at_ms` of the source. On a still it always does.
    pub fn shows(&self, text: &Text, at_ms: u64) -> bool {
        self.still
            || at_ms >= text.start_ms.max(self.range.start_ms)
                && at_ms < text.end_ms.min(self.range.end_ms)
    }

    /// The length of the export: the range after the speed change.
    pub fn output_ms(&self) -> u64 {
        (self.range.len_ms() as f64 / self.speed).round() as u64
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
        if self.framed() {
            return self.set_first_frame(at_ms);
        }
        self.range.start_ms = at_ms.min(self.duration_ms);
        if self.range.end_ms <= self.range.start_ms {
            self.range.end_ms = self.duration_ms;
        }
    }

    /// `]`. An out point at or before the in point moves the in point to the
    /// start.
    pub fn set_end(&mut self, at_ms: u64) {
        if self.framed() {
            return self.set_last_frame(at_ms);
        }
        self.range.end_ms = at_ms.min(self.duration_ms);
        if self.range.end_ms <= self.range.start_ms {
            self.range.start_ms = 0;
        }
    }

    /// A drag on a handle. It stops at the other handle.
    pub fn drag(&mut self, edge: Edge, at_ms: u64) {
        if self.framed() {
            return self.drag_frame_edge(edge, at_ms);
        }
        match edge {
            Edge::Start => self.range.start_ms = at_ms.min(self.range.end_ms),
            Edge::End => {
                self.range.end_ms = at_ms.clamp(self.range.start_ms, self.duration_ms);
            }
        }
    }
}

/// The font of every text. GPUI finds the same file by its family name.
pub const FONT_FILE: &str = "C:/Windows/Fonts/arialbd.ttf";
pub const FONT_FAMILY: &str = "Arial";
/// The line advance of `drawtext`, in ems: the ascent, descent and line gap of
/// Arial Bold.
pub const LINE_HEIGHT: f64 = 1.15;
/// Ascent and descent of Arial Bold, in ems. GPUI puts the rest of the line
/// height half above and half below them.
const ASCENT_DESCENT: f64 = 2288. / 2048.;
/// The padding of the box, in ems.
const BOX_PADDING: f64 = 0.25;
/// Text sizes, as fractions of the frame height.
const DEFAULT_TEXT_SIZE: f64 = 0.08;
const MIN_TEXT_SIZE: f64 = 0.01;
const MAX_TEXT_SIZE: f64 = 1.;

/// Words burnt into the frame. The position and the size are fractions of the
/// output frame, so a change of crop or scale keeps the text in place.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    /// Lines split by `\n`.
    pub words: String,
    /// The center, from 0 to 1 across and down.
    pub x: f64,
    pub y: f64,
    /// The font size over the frame height.
    pub size: f64,
    /// A black box behind the words.
    pub boxed: bool,
    /// Source time, as the range is, so the text stays on its moment when the
    /// range or the speed changes.
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Text {
    /// The words with no space at either end. FFmpeg drops it, so the preview
    /// does too.
    pub fn trimmed(&self) -> &str {
        self.words.trim()
    }

    /// The font size in pixels of a frame `height` high. `drawtext` takes whole
    /// pixels.
    pub fn font_px(&self, height: u32) -> u32 {
        ((self.size * f64::from(height)).round() as u32).max(1)
    }

    pub fn padding_px(font_px: u32) -> u32 {
        (f64::from(font_px) * BOX_PADDING).round() as u32
    }

    /// The center in pixels of a frame `size`.
    pub fn center_px(&self, (width, height): (u32, u32)) -> (f64, f64) {
        (self.x * f64::from(width), self.y * f64::from(height))
    }

    pub fn at(&self, edge: Edge) -> u64 {
        match edge {
            Edge::Start => self.start_ms,
            Edge::End => self.end_ms,
        }
    }

    /// How far below the top of its line GPUI draws the ascent, in pixels.
    /// `drawtext` draws it at the top.
    pub fn ascent_offset_px(font_px: u32) -> f64 {
        f64::from(font_px) * (LINE_HEIGHT - ASCENT_DESCENT) / 2.
    }

    /// A drag of `dx`, `dy` in fractions of the frame from `start`. The
    /// center stays inside the frame.
    pub fn moved(&self, start: (f64, f64), dx: f64, dy: f64) -> (f64, f64) {
        ((start.0 + dx).clamp(0., 1.), (start.1 + dy).clamp(0., 1.))
    }

    /// The wheel. The size grows or shrinks by `factor`, within bounds.
    pub fn scale(&mut self, factor: f64) {
        self.size = (self.size * factor).clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE);
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

/// The speeds the panel offers.
pub const SPEEDS: [f64; 7] = [0.5, 0.75, 1., 1.5, 2., 3., 4.];

pub fn speed_label(speed: f64) -> String {
    format!("{speed}x")
}

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
    Png,
    Jpg,
}

impl Format {
    pub const MOVING: [Format; 4] = [Format::Mp4, Format::Webm, Format::Gif, Format::Webp];
    pub const STILL: [Format; 2] = [Format::Png, Format::Jpg];

    pub fn is_still(self) -> bool {
        matches!(self, Format::Png | Format::Jpg)
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Mp4 => "MP4",
            Format::Webm => "WebM",
            Format::Gif => "GIF",
            Format::Webp => "WebP",
            Format::Png => "PNG",
            Format::Jpg => "JPG",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Mp4 => "mp4",
            Format::Webm => "webm",
            Format::Gif => "gif",
            Format::Webp => "webp",
            Format::Png => "png",
            Format::Jpg => "jpg",
        }
    }

    /// The smallest even frame the encoder takes. NVENC refuses smaller ones:
    /// `h264_nvenc` takes 145 x 49 and up, and `av1_nvenc` 129 x 65.
    pub fn min_size(self) -> (u32, u32) {
        match self {
            Format::Mp4 => (146, 50),
            Format::Webm => (130, 66),
            Format::Gif | Format::Webp | Format::Png | Format::Jpg => (2, 2),
        }
    }

    pub fn has_quality(self) -> bool {
        !matches!(self, Format::Gif | Format::Png)
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
    pub jpg_quality: Quality,
    /// The format last chosen for the other kind of source, a still or not.
    pub other_format: Format,
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
            jpg_quality: Quality::High,
            other_format: Format::Png,
        }
    }
}

impl Controls {
    /// A still takes a still format, and anything else a moving one. Each
    /// kind keeps the format last chosen for it.
    pub fn suit(&mut self, still: bool) {
        if self.format.is_still() != still {
            std::mem::swap(&mut self.format, &mut self.other_format);
        }
    }

    /// The quality of the chosen format. GIF and PNG have none.
    pub fn quality(&self) -> Option<Quality> {
        match self.format {
            Format::Mp4 => Some(self.mp4_quality),
            Format::Webm => Some(self.webm_quality),
            Format::Gif | Format::Png => None,
            Format::Webp => Some(self.webp_quality),
            Format::Jpg => Some(self.jpg_quality),
        }
    }

    pub fn set_quality(&mut self, quality: Quality) {
        match self.format {
            Format::Mp4 => self.mp4_quality = quality,
            Format::Webm => self.webm_quality = quality,
            Format::Gif | Format::Png => {}
            Format::Webp => self.webp_quality = quality,
            Format::Jpg => self.jpg_quality = quality,
        }
    }

    /// The frame rate of the chosen format. MP4 and WebM keep the source rate.
    pub fn fps(&self) -> Option<u32> {
        match self.format {
            Format::Mp4 | Format::Webm | Format::Png | Format::Jpg => None,
            Format::Gif => Some(self.gif_fps),
            Format::Webp => Some(self.webp_fps),
        }
    }

    pub fn set_fps(&mut self, fps: u32) {
        match self.format {
            Format::Mp4 | Format::Webm | Format::Png | Format::Jpg => {}
            Format::Gif => self.gif_fps = fps,
            Format::Webp => self.webp_fps = fps,
        }
    }
}

/// `<source stem> <in>-<out>.<ext>`, for example `cat jump 0m03-0m07.mp4`. A
/// still has no range: `<source stem>.<ext>`. A sequence is `<first stem>
/// sequence.<ext>`. A name the user gave is `<name>.<ext>`.
pub fn export_name(edit: &Edit, format: Format) -> String {
    if let Some(name) = &edit.name {
        return format!("{name}.{}", format.extension());
    }
    let stem = edit
        .source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let range = edit.range;
    if edit.is_sequence() {
        return format!("{stem} sequence.{}", format.extension());
    }
    if format.is_still() {
        return format!("{stem}.{}", format.extension());
    }
    format!(
        "{stem} {}-{}.{}",
        stamp(range.start_ms),
        stamp(range.end_ms),
        format.extension()
    )
}

/// A stem typed for the export, with no character Windows refuses in a file
/// name and no space or dot at the end. `None` when nothing is left.
pub fn clean_name(typed: &str) -> Option<String> {
    let kept: String = typed
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
        .collect();
    let kept = kept.trim_start().trim_end_matches([' ', '.']);
    (!kept.is_empty()).then(|| kept.to_string())
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
    fn the_output_length_is_the_range_after_the_speed() {
        let mut edit = edit();
        edit.set_start(1_000);
        edit.set_end(4_000);
        assert_eq!(edit.output_ms(), 3_000);
        edit.speed = 0.5;
        assert_eq!(edit.output_ms(), 6_000);
        edit.speed = 0.75;
        assert_eq!(edit.output_ms(), 4_000);
        edit.speed = 4.;
        assert_eq!(edit.output_ms(), 750);
        assert_eq!(speed_label(0.75), "0.75x");
        assert_eq!(speed_label(4.), "4x");
    }

    #[test]
    fn a_text_keeps_its_place_and_size_after_a_crop_and_a_scale() {
        let mut edit = edit();
        let at = edit.add_text();
        let text = &mut edit.texts[at];
        (text.x, text.y, text.size) = (0.25, 0.75, 0.1);
        let text = text.clone();
        assert_eq!(text.center_px(edit.output_size()), (480., 810.));
        assert_eq!(text.font_px(edit.output_size().1), 108);

        edit.crop = crop(100, 100, 640, 480);
        assert_eq!(text.center_px(edit.output_size()), (160., 360.));
        assert_eq!(text.font_px(edit.output_size().1), 48);
        edit.max_width = MaxWidth::Pixels(320);
        assert_eq!(edit.output_size(), (320, 240));
        assert_eq!(text.center_px(edit.output_size()), (80., 180.));
        assert_eq!(text.font_px(edit.output_size().1), 24);
        assert_eq!(Text::padding_px(24), 6);
        assert_eq!(text.moved((0.25, 0.75), 0.9, -0.1), (1., 0.65));
    }

    #[test]
    fn a_removed_text_hands_the_selection_to_the_next_or_the_one_before() {
        let mut edit = edit();
        for _ in 0..3 {
            edit.add_text();
        }
        assert_eq!(edit.remove_text(1), Some(1));
        assert_eq!(edit.remove_text(1), Some(0));
        assert_eq!(edit.remove_text(0), None);
        assert_eq!(edit.remove_text(0), None);
    }

    #[test]
    fn a_moved_text_keeps_its_length_inside_the_source() {
        let mut edit = edit();
        let at = edit.add_text();
        edit.texts[at].start_ms = 2_000;
        edit.texts[at].end_ms = 5_000;
        edit.move_text(at, (2_000, 5_000), 1_500);
        assert_eq!(
            (edit.texts[at].start_ms, edit.texts[at].end_ms),
            (3_500, 6_500)
        );
        edit.move_text(at, (2_000, 5_000), -9_000);
        assert_eq!((edit.texts[at].start_ms, edit.texts[at].end_ms), (0, 3_000));
        edit.move_text(at, (2_000, 5_000), 9_000);
        assert_eq!(
            (edit.texts[at].start_ms, edit.texts[at].end_ms),
            (7_000, 10_000)
        );
    }

    #[test]
    fn a_text_shows_only_inside_the_range() {
        let mut edit = edit();
        edit.set_start(2_000);
        edit.set_end(6_000);
        let at = edit.add_text();
        assert_eq!(edit.text_window(&edit.texts[at]), Some((0., 4.)));
        edit.drag_text_edge(at, Edge::Start, 5_000);
        edit.drag_text_edge(at, Edge::End, 9_000);
        assert_eq!(edit.text_window(&edit.texts[at]), Some((3., 4.)));
        edit.drag_text_edge(at, Edge::End, 1_000);
        assert_eq!(edit.texts[at].end_ms, 5_000);
        edit.set_start(7_000);
        assert_eq!(edit.text_window(&edit.texts[at]), None);
    }

    #[test]
    fn the_name_holds_the_stem_and_the_range() {
        let range = Range {
            start_ms: 3_400,
            end_ms: 7_900,
        };
        let mut edit = Edit::new("D:/clips/cat jump.mp4".into(), PathBuf::new(), 0, (0, 0));
        edit.range = range;
        assert_eq!(export_name(&edit, Format::Mp4), "cat jump 0m03-0m07.mp4");
        edit.source = "a.b.webm".into();
        edit.range = Range {
            start_ms: 62_000,
            end_ms: 3_723_000,
        };
        assert_eq!(export_name(&edit, Format::Mp4), "a.b 1m02-1h02m03.mp4");
    }

    #[test]
    fn a_still_name_has_no_range_and_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("cat.png");
        std::fs::write(&source, b"").unwrap();
        let edit = Edit::still(source.clone(), (640, 480));
        let name = export_name(&edit, Format::Jpg);
        assert_eq!(name, "cat.jpg");
        assert_eq!(unique_path(dir.path(), &name), dir.path().join("cat.jpg"));
        let name = export_name(&edit, Format::Png);
        assert_eq!(
            unique_path(dir.path(), &name),
            dir.path().join("cat (2).png")
        );
    }

    #[test]
    fn a_given_name_replaces_the_stem_and_still_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let mut edit = Edit::new("D:/clips/cat.mp4".into(), PathBuf::new(), 9_000, (0, 0));
        edit.name = clean_name(" my: cat?. ");
        assert_eq!(edit.name.as_deref(), Some("my cat"));
        let name = export_name(&edit, Format::Gif);
        assert_eq!(name, "my cat.gif");
        std::fs::write(dir.path().join(&name), b"").unwrap();
        assert_eq!(
            unique_path(dir.path(), &name),
            dir.path().join("my cat (2).gif")
        );
        assert_eq!(clean_name(" ?* . "), None);
    }

    #[test]
    fn a_text_scales_within_bounds() {
        let mut edit = Edit::still("a.png".into(), (640, 480));
        let index = edit.add_text();
        let text = &mut edit.texts[index];
        text.scale(1.5);
        assert!((text.size - DEFAULT_TEXT_SIZE * 1.5).abs() < 1e-12);
        text.scale(1e6);
        assert_eq!(text.size, MAX_TEXT_SIZE);
        text.scale(1e-6);
        assert_eq!(text.size, MIN_TEXT_SIZE);
    }

    #[test]
    fn a_sequence_name_has_the_first_stem() {
        let frame = Frame::new(image::RgbaImage::new(2, 2), 100);
        let edit = Edit::sequence("D:/pics/cat 01.png".into(), vec![frame]);
        assert_eq!(export_name(&edit, Format::Gif), "cat 01 sequence.gif");
    }

    #[test]
    fn each_kind_of_source_keeps_its_own_format() {
        let mut controls = Controls {
            format: Format::Gif,
            ..Controls::default()
        };
        controls.suit(false);
        assert_eq!(controls.format, Format::Gif);
        controls.suit(true);
        assert_eq!(controls.format, Format::Png);
        controls.format = Format::Jpg;
        assert_eq!(controls.quality(), Some(Quality::High));
        assert_eq!(controls.fps(), None);
        controls.suit(false);
        assert_eq!(controls.format, Format::Gif);
        controls.suit(true);
        assert_eq!(controls.format, Format::Jpg);
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
