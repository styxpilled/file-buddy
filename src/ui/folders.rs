//! The breadcrumb of the Folders page. Each part opens its folder.

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{AnyElement, Context, px};
use gpui_kit::component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_kit::component::{ActiveTheme, h_flex};

use crate::ui::FileBuddy;

pub fn breadcrumb(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let mut stops: Vec<(String, Option<PathBuf>)> = vec![("Folders".into(), None)];
    if let Some(dir) = &this.folder
        && let Some(root) = this.config.roots.iter().find(|root| dir.starts_with(root))
    {
        stops.push((display(root), Some(root.clone())));
        let mut at = root.clone();
        for part in dir
            .strip_prefix(root)
            .into_iter()
            .flat_map(Path::components)
        {
            at.push(part);
            let name = part.as_os_str().to_string_lossy().into_owned();
            stops.push((name, Some(at.clone())));
        }
    }

    let view = cx.entity();
    h_flex()
        .w_full()
        .flex_none()
        .h(px(32.))
        .px_3()
        .items_center()
        .text_sm()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            Breadcrumb::new().children(stops.into_iter().map(|(label, folder)| {
                let view = view.clone();
                BreadcrumbItem::new(label).on_click(move |_, window, cx| {
                    let folder = folder.clone();
                    view.update(cx, |this, cx| this.open_folder(folder, window, cx));
                })
            })),
        )
        .into_any_element()
}

/// A path the way Explorer writes it. A root from the config can mix the two
/// separators.
pub fn display(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}
