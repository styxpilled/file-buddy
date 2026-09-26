use crate::video::Video;
use gpui::{
    Element, ElementId, EntityId, Global, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Window,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use yuv::{yuv_nv12_to_bgra, YuvBiPlanarImage, YuvConversionMode, YuvRange, YuvStandardMatrix};

/// A converted frame, held until the decoder presents the next one.
#[derive(Clone)]
struct CachedFrame {
    /// The video the frame came from. An element id outlives the file it shows,
    /// and every file starts its sequence at zero, so the sequence alone let a
    /// paused clip keep the frame of the one before it.
    video: u64,
    seq: u64,
    width: u32,
    height: u32,
    image: Arc<gpui::RenderImage>,
}

/// The frame each video element holds in the sprite atlas. GPUI drops the state
/// of an element that leaves the tree without a word, and the texture of its
/// frame would stay in the atlas for the life of the window.
#[derive(Default)]
struct Shown {
    frames: HashMap<EntityId, Arc<gpui::RenderImage>>,
    painted: HashSet<EntityId>,
}

impl Global for Shown {}

/// Free the frames of the video elements that were not painted in the last
/// frame. Call it once per frame, before the videos paint.
pub fn release_unused(window: &mut Window, cx: &mut gpui::App) {
    let Some(shown) = cx.try_global::<Shown>() else {
        return;
    };
    if shown.frames.len() == shown.painted.len() {
        cx.global_mut::<Shown>().painted.clear();
        return;
    }
    let shown = cx.global_mut::<Shown>();
    let painted = std::mem::take(&mut shown.painted);
    let gone: Vec<_> = shown
        .frames
        .extract_if(|id, _| !painted.contains(id))
        .map(|(_, image)| image)
        .collect();
    for image in gone {
        cx.drop_image(image, Some(window));
    }
}

pub struct VideoElement {
    video: Video,
    display_width: Option<gpui::Pixels>,
    display_height: Option<gpui::Pixels>,
    element_id: Option<ElementId>,
}

impl VideoElement {
    pub fn new(video: Video) -> Self {
        Self {
            video,
            display_width: None,
            display_height: None,
            element_id: None,
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.element_id = Some(id.into());
        self
    }

    pub fn size(mut self, width: gpui::Pixels, height: gpui::Pixels) -> Self {
        self.display_width = Some(width);
        self.display_height = Some(height);
        self
    }

    pub fn width(mut self, width: gpui::Pixels) -> Self {
        self.display_width = Some(width);
        self.display_height = None;
        self
    }

    pub fn height(mut self, height: gpui::Pixels) -> Self {
        self.display_height = Some(height);
        self.display_width = None;
        self
    }

    pub fn buffer_capacity(self, capacity: usize) -> Self {
        self.video.set_frame_buffer_capacity(capacity);
        self
    }

    fn get_display_size(&self) -> (gpui::Pixels, gpui::Pixels) {
        match (self.display_width, self.display_height) {
            (Some(w), Some(h)) => (w, h),
            _ => {
                let (w, h) = self.video.display_size();
                (gpui::px(w as f32), gpui::px(h as f32))
            }
        }
    }

    fn fitted_bounds(
        &self,
        bounds: gpui::Bounds<gpui::Pixels>,
        frame_width: u32,
        frame_height: u32,
    ) -> gpui::Bounds<gpui::Pixels> {
        let container_w: f32 = bounds.size.width.into();
        let container_h: f32 = bounds.size.height.into();
        let frame_w = frame_width as f32;
        let frame_h = frame_height as f32;

        let scale = if frame_w > 0.0 && frame_h > 0.0 {
            (container_w / frame_w).min(container_h / frame_h)
        } else {
            1.0
        };

        let dest_w = (frame_w * scale).max(0.0);
        let dest_h = (frame_h * scale).max(0.0);
        let offset_x = (container_w - dest_w) * 0.5;
        let offset_y = (container_h - dest_h) * 0.5;

        gpui::Bounds::new(
            gpui::point(
                bounds.origin.x + gpui::px(offset_x),
                bounds.origin.y + gpui::px(offset_y),
            ),
            gpui::size(gpui::px(dest_w), gpui::px(dest_h)),
        )
    }

    /// Converts the current frame and keeps the result until a new frame is
    /// presented. Without this every window frame re-ran the colour conversion
    /// and uploaded a new texture for every video on screen, paused ones
    /// included.
    fn cached_image(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::App,
        seq: u64,
    ) -> Option<CachedFrame> {
        use image::{ImageBuffer, Rgba};
        use smallvec::SmallVec;

        let cache: gpui::Entity<Option<CachedFrame>> = window.use_state(cx, |_, _| None);
        let id = self.video.id();
        let key = cache.entity_id();
        cx.default_global::<Shown>().painted.insert(key);

        if let Some(hit) = cache
            .read(cx)
            .clone()
            .filter(|c| c.video == id && c.seq == seq)
        {
            cx.global_mut::<Shown>()
                .frames
                .entry(key)
                .or_insert_with(|| hit.image.clone());
            return Some(hit);
        }

        let (yuv_data, frame_width, frame_height) = self.video.current_frame_data()?;
        let rgb_data = self.yuv_to_rgb(&yuv_data, frame_width, frame_height);
        let image_buffer =
            ImageBuffer::<Rgba<u8>, _>::from_raw(frame_width, frame_height, rgb_data)?;

        let frames: SmallVec<[image::Frame; 1]> =
            SmallVec::from_elem(image::Frame::new(image_buffer), 1);
        let fresh = CachedFrame {
            video: id,
            seq,
            width: frame_width,
            height: frame_height,
            image: Arc::new(gpui::RenderImage::new(frames)),
        };

        let previous = cache.update(cx, |this, _| this.replace(fresh.clone()));
        if let Some(previous) = previous {
            cx.drop_image(previous.image, Some(window));
        }
        cx.default_global::<Shown>()
            .frames
            .insert(key, fresh.image.clone());

        Some(fresh)
    }

    fn yuv_to_rgb(&self, yuv_data: &[u8], width: u32, height: u32) -> Vec<u8> {
        let width_usize = width as usize;
        let height_usize = height as usize;
        let y_size = width_usize * height_usize;
        let uv_size = (width_usize * height_usize) / 2;

        if yuv_data.len() < y_size + uv_size {
            return vec![0; width_usize * height_usize * 4];
        }

        let y_plane = &yuv_data[..y_size];
        let uv_plane = &yuv_data[y_size..y_size + uv_size];

        let yuv_bi_planar = YuvBiPlanarImage {
            y_plane,
            y_stride: width,
            uv_plane,
            uv_stride: width,
            width,
            height,
        };

        let mut bgra = vec![0u8; width_usize * height_usize * 4];
        let rgba_stride = width * 4;

        if yuv_nv12_to_bgra(
            &yuv_bi_planar,
            &mut bgra,
            rgba_stride,
            YuvRange::Full,
            YuvStandardMatrix::Bt709,
            YuvConversionMode::Balanced,
        )
        .is_ok()
        {
            return bgra;
        }

        if yuv_nv12_to_bgra(
            &yuv_bi_planar,
            &mut bgra,
            rgba_stride,
            YuvRange::Limited,
            YuvStandardMatrix::Bt709,
            YuvConversionMode::Balanced,
        )
        .is_ok()
        {
            return bgra;
        }

        match yuv_nv12_to_bgra(
            &yuv_bi_planar,
            &mut bgra,
            rgba_stride,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
            YuvConversionMode::Balanced,
        ) {
            Ok(_) => bgra,
            Err(_) => vec![0; width_usize * height_usize * 4],
        }
    }
}

impl Element for VideoElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        self.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (mut width, mut height) = self.get_display_size();

        if self.display_width.is_none() || self.display_height.is_none() {
            let (vw, vh) = self.video.display_size();
            if self.display_width.is_none() {
                width = gpui::px(vw as f32);
            }
            if self.display_height.is_none() {
                height = gpui::px(vh as f32);
            }
        }

        let style = gpui::Style {
            size: gpui::Size {
                width: gpui::Length::Definite(gpui::DefiniteLength::Absolute(
                    gpui::AbsoluteLength::Pixels(width),
                )),
                height: gpui::Length::Definite(gpui::DefiniteLength::Absolute(
                    gpui::AbsoluteLength::Pixels(height),
                )),
            },
            ..Default::default()
        };

        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout_state: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        // Only a playing video needs the next frame. A wall of paused thumbnails
        // would otherwise hold the window at the display refresh rate forever.
        // A video that starts paused is the exception: it must keep asking until
        // the decoder presents its first frame, or it stays blank.
        // A seek on a paused video is the same.
        if !self.video.paused() || self.video.frame_seq() == 0 || self.video.seeking() {
            window.request_animation_frame();
        }
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout_state: &mut Self::RequestLayoutState,
        _prepaint_state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let seq = self.video.frame_seq();
        let Some(frame) = self.cached_image(window, cx, seq) else {
            return;
        };

        // `bounds` clips, `image_bounds` places and scales. The frame is already
        // fitted to `dest_bounds`, so the two are the same rectangle.
        let dest_bounds = self.fitted_bounds(bounds, frame.width, frame.height);
        window
            .paint_image(
                dest_bounds,
                dest_bounds,
                gpui::Corners::default(),
                frame.image,
                0,
                false,
            )
            .ok();
    }
}

impl IntoElement for VideoElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub fn video(video: Video) -> VideoElement {
    VideoElement::new(video)
}
