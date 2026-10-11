//! The result grid. A few thousand tiles cannot all be elements, so the rows
//! come from a virtual list and only the visible ones are built.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, Context, px, size};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex, v_virtual_list};

use super::{autoscroll, table, tile};
use crate::ui::{FileBuddy, Page};

pub const TILE_WIDTH: f32 = 220.;
pub const TILE_HEIGHT: f32 = 124.;
/// The tile, its two lines of text and the gap below it.
pub const ROW_HEIGHT: f32 = TILE_HEIGHT + 58.;
const GAP: f32 = 12.;
const PADDING: f32 = 12.;

/// How many tiles fit across a grid of this width.
pub fn columns(width: f32) -> usize {
    let usable = width - PADDING * 2.;
    (((usable + GAP) / (TILE_WIDTH + GAP)).floor() as usize).max(1)
}

pub fn grid(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    if this.results.is_empty() {
        return v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(match this.page() {
                Page::Review => "Nothing to review in this group.",
                Page::Folders if !this.listing => "Nothing in this folder matches.",
                Page::Folders if this.folder.is_some() || this.dropped => "This folder is empty.",
                _ => "Nothing here. Check the roots in file-buddy.toml.",
            })
            .into_any_element();
    }
    if this.table_shown() {
        return table::table(this, cx);
    }

    let columns = this.columns.max(1);
    let rows = this.results.len().div_ceil(columns);
    let sizes = Rc::new(vec![size(px(f32::MAX), px(ROW_HEIGHT)); rows]);
    let scroll = this.scroll.clone();

    let list = v_virtual_list(cx.entity(), "grid", sizes, move |this, range, _, cx| {
        range
            .map(|row| {
                let first = row * columns;
                let last = (first + columns).min(this.results.len());
                h_flex()
                    .w_full()
                    .gap(px(GAP))
                    .px(px(PADDING))
                    .children(
                        (first..last)
                            .map(|index| tile::tile(this, index, cx))
                            .collect::<Vec<_>>(),
                    )
                    .into_any_element()
            })
            .collect()
    })
    .track_scroll(&scroll);
    autoscroll::attach(this, list, cx)
        .vertical_scrollbar(&*scroll)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_decides_the_columns() {
        assert_eq!(columns(900.), 3);
        assert_eq!(columns(1400.), 5);
        // Never zero, whatever the window does.
        assert_eq!(columns(0.), 1);
        assert_eq!(columns(120.), 1);
    }
}
