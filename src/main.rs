//! The application. It opens the database, opens one window and gets out of the
//! way. Everything it shows comes from the library.

use anyhow::Result;
use file_buddy::db::Db;
use file_buddy::ui::{self, FileBuddy};
use file_buddy::{Config, paths};
use gpui::prelude::*;
use gpui::{App, WindowOptions, px, size};
use gpui_kit::assets::Assets;
use gpui_kit::component::{Root, Theme, ThemeMode};

fn main() -> Result<()> {
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
            ui::bind_keys(cx);

            let options = WindowOptions {
                focus: true,
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
                    None,
                    size(px(1280.), px(800.)),
                    cx,
                ))),
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
