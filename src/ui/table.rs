//! The table view: one line per entry, no pictures. The selection, the marks
//! and the keys work as they do in the grid, with one column.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, Hsla, div, px, transparent_black};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex, v_virtual_list};

use super::{autoscroll, tile, when};
use crate::db::files::{FileRow, State};
use crate::media;
use crate::search::folder::{Entry, Stray};
use crate::ui::FileBuddy;

const ROW_HEIGHT: f32 = 26.;
/// Kind, length, size, modified and tags, or in Review why the file is there.
/// The name takes what is left.
const WIDTHS: [f32; 5] = [56., 56., 72., 120., 150.];

pub fn table(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let sizes = Rc::new(vec![
        gpui::size(px(f32::MAX), px(ROW_HEIGHT));
        this.results.len()
    ]);
    let muted = cx.theme().muted_foreground;
    let last = if this.review().is_some() {
        "Why"
    } else {
        "Tags"
    };
    let [kind, length, size, modified, tags] =
        ["Kind", "Length", "Size", "Modified", last].map(|label| text(label, muted));
    let header = [kind, length.text_right(), size.text_right(), modified, tags];
    v_flex()
        .size_full()
        .child(
            line(text("Name", muted), header)
                .border_b_1()
                .border_color(cx.theme().border),
        )
        .child(
            div().flex_1().min_h_0().child(
                autoscroll::attach(
                    this,
                    v_virtual_list(cx.entity(), "table", sizes, |this, range, _, cx| {
                        range.map(|index| row(this, index, cx)).collect()
                    })
                    .track_scroll(&this.scroll),
                    cx,
                )
                .vertical_scrollbar(&*this.scroll),
            ),
        )
        .into_any_element()
}

fn row(this: &FileBuddy, index: usize, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(entry) = this.results.get(index) else {
        return div().into_any_element();
    };
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let marked = entry.hit().is_some_and(|hit| this.is_marked(hit));
    let background = match (index == this.selected, marked) {
        (true, _) => theme.primary.opacity(0.2),
        (false, true) => theme.success.opacity(0.15),
        (false, false) => transparent_black(),
    };

    let (name, kind) = match entry {
        Entry::Folder(path) => (
            h_flex()
                .gap_1()
                .min_w_0()
                .child(Icon::new(IconName::Folder).xsmall().text_color(muted))
                .child(text(&this.folder_name(path), theme.foreground))
                .into_any_element(),
            text("folder", muted),
        ),
        Entry::File(hit) if hit.file.state == State::Pending => (
            text(&hit.file.filename, theme.warning).into_any_element(),
            text("pending", theme.warning),
        ),
        Entry::File(hit) => (
            text(&hit.file.filename, theme.foreground).into_any_element(),
            text(hit.file.kind.label(), muted),
        ),
        Entry::Stray { path, why, .. } => {
            let color = tile::stray_color(*why, cx);
            (
                text(&FileRow::name_of(path), color).into_any_element(),
                text(stray_label(*why), color),
            )
        }
    };
    let hit = entry.hit();
    let length = hit
        .and_then(|hit| hit.file.duration)
        .map(media::duration)
        .unwrap_or_default();
    let last = match hit {
        Some(hit) if this.review().is_some() => text(&hit.matched, muted),
        Some(hit) => text(
            &this
                .row_tags
                .get(&hit.file.hash)
                .map(|names| names.join(", "))
                .unwrap_or_default(),
            theme.primary,
        ),
        None => div(),
    };

    line(
        name,
        [
            kind,
            text(&length, muted).text_right(),
            text(&entry.size().map(media::bytes).unwrap_or_default(), muted).text_right(),
            text(
                &entry.mtime().map(when::local_time).unwrap_or_default(),
                muted,
            ),
            last,
        ],
    )
    .id(("row", index))
    .bg(background)
    .cursor_pointer()
    .on_click(
        cx.listener(move |this, event, window, cx| tile::click(this, index, event, window, cx)),
    )
    .into_any_element()
}

/// One line of the table, the header included, so the columns line up.
fn line(name: impl IntoElement, rest: [Div; 5]) -> Div {
    h_flex()
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex_none()
        .px_3()
        .gap_3()
        .items_center()
        .text_xs()
        .child(div().flex_1().min_w_0().overflow_hidden().child(name))
        .children(
            rest.into_iter()
                .zip(WIDTHS)
                .map(|(cell, width)| cell.flex_none().w(px(width))),
        )
}

/// The kind column is narrow, so it says in one word what a tile says in a
/// phrase.
fn stray_label(why: Stray) -> &'static str {
    match why {
        Stray::Unindexed => "unindexed",
        Stray::Ignored => "ignored",
        Stray::Unmatched => "not media",
    }
}

fn text(text: &str, color: Hsla) -> Div {
    div().truncate().text_color(color).child(text.to_string())
}
