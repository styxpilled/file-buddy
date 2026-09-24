//! The application. It opens the database, opens one window and gets out of the
//! way. Everything it shows comes from the library.

// A console program gets a console window when Explorer starts it.
#![windows_subsystem = "windows"]

use anyhow::Result;
use file_buddy::config::Config;
use file_buddy::db::Db;
use file_buddy::paths;
use file_buddy::ui::{self, FileBuddy};
use gpui::{App, WindowOptions, px, size};
use gpui::{TitlebarOptions, prelude::*};
use gpui_kit::assets::Assets;
use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::component::{Root, Theme, ThemeMode};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

fn main() -> Result<()> {
    // Started from a terminal, as `cargo run` does, the log goes there. Started
    // from Explorer there is no console to attach to, and the call fails.
    // SAFETY: nothing holds a handle to a console yet.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.ok();
    env_logger::init();

    let config = Config::load_or_create(&paths::config_path()?)?;
    let db_path = paths::database_path()?;
    let cache = paths::thumbnail_dir()?;
    // Fail here, with a message, rather than in a window that cannot show one.
    Db::open(&db_path)?;

    gpui_kit::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            Theme::set_scrollbar_mode(ScrollbarMode::Always, cx);
            for problem in ui::bind_keys(&config.keys, cx) {
                log::warn!("[keys]: {problem}");
            }

            let options = WindowOptions {
                focus: true,
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
                    None,
                    size(px(1280.), px(800.)),
                    cx,
                ))),
                // The header is the title bar, with its own window buttons.
                titlebar: Some(TitlebarOptions {
                    title: Some("file-buddy".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                ..Default::default()
            };
            cx.open_window(options, |window, cx| {
                let db = Db::open(&db_path).expect("failed to open the database");
                let view = cx.new(|cx| FileBuddy::new(db, &config, &db_path, &cache, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open the window");
            cx.activate(true);
        });
    Ok(())
}
