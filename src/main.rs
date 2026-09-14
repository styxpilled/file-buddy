//! Test application for issue 02. It proves the vendored `gpui-video` player
//! under `gpui-kit`: several instances in one window, each looping, muted and
//! started by hover.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    App, Context, Entity, Render, SharedString, Window, WindowOptions, div, prelude::*, px, rgb,
};
use gpui_kit::assets::Assets;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{IconName, Root, Sizable};
use gpui_video::{Video, video};
use url::Url;

/// Background behind the letterboxed video.
const BACKGROUND: u32 = 0x151718;
const TILE_WIDTH: f32 = 320.;
const TILE_HEIGHT: f32 = 180.;
/// `w` and `h` size the border box, so the video gets the content box or it
/// paints over its own border.
const TILE_BORDER: f32 = 2.;
const MAX_TILES: usize = 6;
/// How often the status line is redrawn. A frozen counter means a stalled decoder.
const STATUS_INTERVAL: Duration = Duration::from_millis(250);
const SEEK_STEP: Duration = Duration::from_secs(5);

struct Tile {
    /// Distinct per tile, so every element id in the window is distinct.
    index: usize,
    name: SharedString,
    video: Video,
    hovered: bool,
    rate: Rate,
}

/// Separates a slow window from a slow decoder: `renders` is how often the
/// window drew this tile, `frames` is how many frames the decoder presented.
#[derive(Default)]
struct Rate {
    renders: u32,
    last_seq: u64,
    sampled_at: Option<Instant>,
    renders_per_second: u32,
    frames_per_second: u32,
}

impl Rate {
    fn tick(&mut self, seq: u64) {
        self.renders += 1;
        let sampled_at = self.sampled_at.get_or_insert_with(Instant::now);
        if sampled_at.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.renders_per_second = self.renders;
        self.frames_per_second = (seq - self.last_seq) as u32;
        self.renders = 0;
        self.last_seq = seq;
        self.sampled_at = Some(Instant::now());
    }
}

impl Tile {
    fn new(index: usize, path: &Path) -> Result<Self, String> {
        let uri = Url::from_file_path(path).map_err(|_| format!("not absolute: {path:?}"))?;
        let video = Video::new(&uri).map_err(|e| format!("{}: {e}", path.display()))?;
        video.set_looping(true);
        video.set_muted(true);
        video.set_paused(true);
        Ok(Self {
            index,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
                .into(),
            video,
            hovered: false,
            rate: Rate::default(),
        })
    }

    fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.hovered == hovered {
            return;
        }
        self.hovered = hovered;
        if hovered {
            self.video.set_paused(false);
        } else {
            self.video.set_paused(true);
            // Rewind, so the next hover replays the clip from the start.
            self.video.seek(Duration::ZERO, false).ok();
        }
        cx.notify();
    }

    fn status(&self) -> String {
        let position = self.video.position().as_secs_f32();
        let duration = self.video.duration().as_secs_f32();
        format!(
            "{position:>5.1} / {duration:.1}s  {:>2} draw  {:>2} fps  {}{}{}",
            self.rate.renders_per_second,
            self.rate.frames_per_second,
            if self.video.paused() { "paused" } else { "playing" },
            if self.video.muted() { " muted" } else { "" },
            if self.video.looping() { " loop" } else { "" },
        )
    }
}

impl Render for Tile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rate.tick(self.video.frame_seq());

        div()
            .id(("tile", self.index))
            .flex()
            .flex_col()
            .gap_1()
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                this.set_hovered(*hovered, cx)
            }))
            .child(
                div()
                    .w(px(TILE_WIDTH))
                    .h(px(TILE_HEIGHT))
                    .bg(rgb(BACKGROUND))
                    .border_2()
                    .border_color(rgb(if self.hovered { 0x4f8cff } else { 0x2a2d2e }))
                    .overflow_hidden()
                    .child(
                        video(self.video.clone())
                            .id(("video", self.index))
                            .size(
                                px(TILE_WIDTH - TILE_BORDER * 2.),
                                px(TILE_HEIGHT - TILE_BORDER * 2.),
                            )
                            .buffer_capacity(30),
                    ),
            )
            .child(
                div()
                    .w(px(TILE_WIDTH))
                    .text_xs()
                    .text_color(rgb(0xb0b6b8))
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .w(px(TILE_WIDTH))
                    .text_xs()
                    .text_color(rgb(0x7f8688))
                    .child(self.status()),
            )
    }
}

struct Wall {
    tiles: Vec<Entity<Tile>>,
    errors: Vec<String>,
}

impl Wall {
    fn new(paths: &[PathBuf], cx: &mut Context<Self>) -> Self {
        let mut tiles = Vec::new();
        let mut errors = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            match Tile::new(index, path) {
                Ok(tile) => tiles.push(cx.new(|_| tile)),
                Err(e) => errors.push(e),
            }
        }
        // The status lines read state owned by the decoder threads, so they need
        // their own redraw tick.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(STATUS_INTERVAL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self { tiles, errors }
    }

    fn for_each_video(&self, cx: &mut Context<Self>, f: impl Fn(&Video)) {
        for tile in &self.tiles {
            f(&tile.read(cx).video);
        }
        for tile in &self.tiles {
            tile.update(cx, |_, cx| cx.notify());
        }
    }

    fn toggle_muted(&mut self, cx: &mut Context<Self>) {
        let muted = self.first_video(cx).map(|v| v.muted()).unwrap_or(true);
        self.for_each_video(cx, |v| v.set_muted(!muted));
    }

    fn toggle_looping(&mut self, cx: &mut Context<Self>) {
        let looping = self.first_video(cx).map(|v| v.looping()).unwrap_or(false);
        self.for_each_video(cx, |v| v.set_looping(!looping));
    }

    fn seek_forward(&mut self, cx: &mut Context<Self>) {
        self.for_each_video(cx, |v| {
            let target = (v.position() + SEEK_STEP).min(v.duration());
            v.seek(target, false).ok();
        });
    }

    fn rewind(&mut self, cx: &mut Context<Self>) {
        self.for_each_video(cx, |v| {
            v.seek(Duration::ZERO, false).ok();
        });
    }

    fn first_video(&self, cx: &Context<Self>) -> Option<Video> {
        self.tiles.first().map(|t| t.read(cx).video.clone())
    }
}

impl Render for Wall {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = self.first_video(cx).map(|v| v.muted()).unwrap_or(true);
        let looping = self.first_video(cx).map(|v| v.looping()).unwrap_or(false);

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(0x0e1011))
            .text_color(rgb(0xe6eaeb))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("rewind")
                            .small()
                            .icon(IconName::RotateCw)
                            .label("Rewind")
                            .on_click(cx.listener(|this, _, _window, cx| this.rewind(cx))),
                    )
                    .child(
                        Button::new("seek")
                            .small()
                            .icon(IconName::Play)
                            .label("Seek +5s")
                            .on_click(cx.listener(|this, _, _window, cx| this.seek_forward(cx))),
                    )
                    .child(
                        Button::new("mute")
                            .small()
                            .label(if muted { "Unmute" } else { "Mute" })
                            .on_click(cx.listener(|this, _, _window, cx| this.toggle_muted(cx))),
                    )
                    .child(
                        Button::new("loop")
                            .small()
                            .primary()
                            .label(if looping { "Loop on" } else { "Loop off" })
                            .on_click(cx.listener(|this, _, _window, cx| this.toggle_looping(cx))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x7f8688))
                            .child("Hover a tile to play it."),
                    ),
            )
            .children(
                self.errors
                    .iter()
                    .map(|e| div().text_xs().text_color(rgb(0xff6b6b)).child(e.clone())),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .children(self.tiles.iter().cloned()),
            )
    }
}

/// Every video file directly under `root`, recursively, up to `MAX_TILES`.
fn collect_videos(root: &Path) -> Vec<PathBuf> {
    const EXTENSIONS: [&str; 4] = ["mp4", "mov", "mkv", "webm"];
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found.truncate(MAX_TILES);
    found
}

fn main() {
    env_logger::init();

    let arg = std::env::args().nth(1).unwrap_or_else(|| "test_videos".into());
    let root = std::env::current_dir()
        .expect("failed to read current directory")
        .join(arg);
    let paths = collect_videos(&root);
    if paths.is_empty() {
        eprintln!("no video files under {}", root.display());
        return;
    }

    gpui_kit::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            cx.open_window(
                WindowOptions {
                    focus: true,
                    ..Default::default()
                },
                |window, cx| {
                    let wall = cx.new(|cx| Wall::new(&paths, cx));
                    cx.new(|cx| Root::new(wall, window, cx))
                },
            )
            .expect("failed to open window");
            cx.activate(true);
        });
}
