use gpui::{
    App, Application, Context, Pixels, Render, Window, WindowOptions, div, prelude::*, px, rgb,
};
use gpui_video::{Video, video};
use url::Url;

/// Background behind the letterboxed video.
const BACKGROUND: u32 = 0x151718;

struct VideoPlayer {
    video: Video,
}

impl VideoPlayer {
    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        self.video.set_paused(!self.video.paused());
        cx.notify();
    }

    /// Largest size with the video's aspect ratio that fits inside `available`.
    fn fitted_size(&self, available: gpui::Size<Pixels>) -> (Pixels, Pixels) {
        let aspect = self.video.aspect_ratio();
        let (max_w, max_h) = (f32::from(available.width), f32::from(available.height));
        if aspect <= 0.0 || max_w <= 0.0 || max_h <= 0.0 {
            return (px(0.), px(0.));
        }
        // Match the window's width, unless that would overflow its height.
        if max_w / max_h > aspect {
            (px(max_h * aspect), px(max_h))
        } else {
            (px(max_w), px(max_w / aspect))
        }
    }
}

impl Render for VideoPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (width, height) = self.fitted_size(window.viewport_size());

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(BACKGROUND))
            .child(
                div()
                    .id("video-container")
                    .w(width)
                    .h(height)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _event, _window, cx| this.toggle_playback(cx)))
                    .child(
                        video(self.video.clone())
                            .id("main-video")
                            .size(width, height)
                            .buffer_capacity(30),
                    ),
            )
    }
}

fn main() {
    env_logger::init();
    Application::new().run(|cx: &mut App| {
        // `Url::from_file_path` requires an absolute path, so resolve it against the cwd.
        let arg = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "test_videos/prod moment.mp4".into());
        let path = std::env::current_dir()
            .expect("failed to read current directory")
            .join(arg);
        let uri = Url::from_file_path(&path)
            .unwrap_or_else(|_| panic!("invalid file path: {}", path.display()));

        cx.open_window(
            WindowOptions {
                focus: true,
                ..Default::default()
            },
            |_, cx| {
                let video = Video::new(&uri).expect("failed to create video");
                // Start paused; playback begins on the first click.
                video.set_paused(true);
                cx.new(|_| VideoPlayer { video })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
