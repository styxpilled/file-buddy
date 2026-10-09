//! The application. It opens the database, opens one window and gets out of the
//! way. Everything it shows comes from the library.

// A console program gets a console window when Explorer starts it.
#![windows_subsystem = "windows"]

use anyhow::Result;
use file_buddy::config::Config;
use file_buddy::db::Db;
use file_buddy::paths;
use file_buddy::ui::{self, FileBuddy};
use gpui::{App, AssetSource, SharedString, WindowOptions, px, size};
use gpui::{TitlebarOptions, prelude::*};
use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::component::{Root, Theme, ThemeMode};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Written out when it drops. The application can quit without returning from
/// `run`, so quitting drops it too.
#[cfg(feature = "dhat-heap")]
static PROFILER: std::sync::Mutex<Option<dhat::Profiler>> = std::sync::Mutex::new(None);

// The icons beyond the ones gpui-kit embeds for its components.
icon_assets!(ExtraIcons, [Trash, Shredder]);

struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn main() -> Result<()> {
    #[cfg(feature = "dhat-heap")]
    {
        *PROFILER.lock().unwrap() = Some(dhat::Profiler::builder().trim_backtraces(None).build());
    }
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
        .with_assets(AppAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            #[cfg(feature = "dhat-heap")]
            cx.on_app_quit(|_| {
                PROFILER.lock().unwrap().take();
                async {}
            })
            .detach();
            Theme::change(ThemeMode::Dark, None, cx);
            Theme::set_scrollbar_mode(ScrollbarMode::Always, cx);
            for problem in ui::bind_keys(&config.keys, cx) {
                log::warn!("[keys]: {problem}");
            }

            let options = WindowOptions {
                // Simulated input works behind the user's windows.
                focus: !cfg!(feature = "sim"),
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
                    None,
                    size(px(1335.), px(800.)),
                    cx,
                ))),
                // The header is the title bar, with its own window buttons.
                titlebar: Some(TitlebarOptions {
                    // A debug build says so, so scripts never pick the
                    // user's release window by mistake.
                    title: Some(if cfg!(debug_assertions) {
                        "file-buddy dev".into()
                    } else {
                        "file-buddy".into()
                    }),
                    appears_transparent: true,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let window = cx
                .open_window(options, |window, cx| {
                    let db = Db::open(&db_path).expect("failed to open the database");
                    let view =
                        cx.new(|cx| FileBuddy::new(db, &config, &db_path, &cache, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                })
                .expect("failed to open the window");
            #[cfg(feature = "sim")]
            ui::sim::serve(window.into(), cx);
            #[cfg(not(feature = "sim"))]
            {
                let _ = window;
                cx.activate(true);
            }
        });
    #[cfg(feature = "dhat-heap")]
    PROFILER.lock().unwrap().take();
    Ok(())
}
