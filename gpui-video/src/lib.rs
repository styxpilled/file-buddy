//! A video player element for [gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui),
//! built on FFmpeg. Vendored for file-buddy and Windows only: `build.rs` copies
//! the DLLs from `ffmpeg/bin` next to the built binaries.
//!
//! A [`Video`] owns a decoder thread and a cpal output stream. The thread
//! demuxes, decodes and paces both streams against a master clock, and hands
//! finished NV12 frames to the handle. [`video`] wraps a handle in an element
//! that converts the current frame to BGRA and paints it, once per presented
//! frame.
//!
//! # Example
//!
//! ```no_run
//! use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};
//! use gpui_video::{Video, video};
//! use std::path::PathBuf;
//! use url::Url;
//!
//! struct VideoPlayer {
//!     video: Video,
//! }
//!
//! impl VideoPlayer {
//!     fn open(path: &str) -> Self {
//!         let uri = Url::from_file_path(PathBuf::from(path)).expect("invalid file path");
//!         Self {
//!             video: Video::new(&uri).expect("failed to create video"),
//!         }
//!     }
//! }
//!
//! impl Render for VideoPlayer {
//!     fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
//!         div()
//!             .size_full()
//!             .child(video(self.video.clone()).id("main-video").buffer_capacity(30))
//!     }
//! }
//! ```
//!
//! # Playback control
//!
//! ```no_run
//! # use gpui_video::Video;
//! # use url::Url;
//! # let uri = Url::parse("file:///video.mp4").unwrap();
//! # let video = Video::new(&uri).unwrap();
//! use std::time::Duration;
//!
//! video.set_paused(true);
//! video.seek(Duration::from_secs(30), false).ok();
//! video.set_volume(0.5);
//! video.set_speed(1.5).ok();
//! ```

mod element;
mod error;
mod stretch;
mod video;

pub use element::{video, VideoElement};
pub use error::Error;
pub use video::{Position, Video, VideoOptions};

// Re-export commonly used types
pub use url::Url;
