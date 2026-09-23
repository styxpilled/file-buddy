//! The bar of the Folders page: the breadcrumb, where each part opens its
//! folder, and the sort buttons.

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};
use gpui_kit::component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex};

use crate::search::folder::Sort;
use crate::ui::{FileBuddy, with_pointer};

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
        .child(div().flex_1())
        // The sort only orders a folder. A query keeps its ranking, so the
        // sort buttons go while one is typed.
        .when(this.listing, |bar| {
            bar.child(h_flex().gap_1().children(Sort::ALL.map(|sort| {
                toggle(
                    sort.label(),
                    "S",
                    this.sort == sort,
                    cx.listener(move |this, _, window, cx| this.set_sort(sort, window, cx)),
                )
            })))
        })
        .into_any_element()
}

pub(super) fn toggle(
    label: &'static str,
    key: &'static str,
    on: bool,
    click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::Div {
    with_pointer(
        Button::new(label)
            .xsmall()
            .label(label)
            .tooltip(key)
            .selected(on)
            .when(on, |button| button.primary())
            .on_click(click),
        true,
    )
}

/// A path the way Explorer writes it. A root from the config can mix the two
/// separators.
pub fn display(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}
