//! The table view: one line per entry, no pictures. The selection, the marks
//! and the keys work as they do in the grid, with one column.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, Hsla, div, px, transparent_black};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex, v_virtual_list};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

use super::tile;
use crate::db::files::{FileRow, State};
use crate::media;
use crate::search::folder::Entry;
use crate::ui::FileBuddy;

const ROW_HEIGHT: f32 = 26.;
/// Kind, length, size, modified and tags, or in Review why the file is there.
/// The name takes what is left.
const WIDTHS: [f32; 5] = [110., 56., 72., 120., 150.];

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
                v_virtual_list(cx.entity(), "table", sizes, |this, range, _, cx| {
                    range.map(|index| row(this, index, cx)).collect()
                })
                .track_scroll(&this.scroll),
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
            text("not probed yet", theme.warning),
        ),
        Entry::File(hit) => (
            text(&hit.file.filename, theme.foreground).into_any_element(),
            text(hit.file.kind.label(), muted),
        ),
        Entry::Stray { path, why, .. } => {
            let color = tile::stray_color(*why, cx);
            (
                text(&FileRow::name_of(path), color).into_any_element(),
                text(why.as_str(), color),
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
            text(&entry.size().map(bytes).unwrap_or_default(), muted).text_right(),
            text(&entry.mtime().map(local_time).unwrap_or_default(), muted),
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

fn text(text: &str, color: Hsla) -> Div {
    div().truncate().text_color(color).child(text.to_string())
}

fn bytes(size: u64) -> String {
    const KB: f64 = 1024.;
    const MB: f64 = KB * 1024.;
    const GB: f64 = MB * 1024.;
    let size = size as f64;
    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else {
        format!("{:.0} KB", (size / KB).ceil())
    }
}

/// Seconds since the Unix epoch, as a date and time in the zone of this
/// machine, with the daylight saving rule of that date.
fn local_time(seconds: i64) -> String {
    // A FILETIME counts 100 ns steps from 1601.
    const EPOCH_GAP: i64 = 11_644_473_600;
    let ticks = ((seconds + EPOCH_GAP) as u64) * 10_000_000;
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: both calls only read and write the structs passed to them.
    let converted = unsafe {
        FileTimeToSystemTime(&file_time, &mut utc)
            .and_then(|()| SystemTimeToTzSpecificLocalTime(None, &utc, &mut local))
    };
    if converted.is_err() {
        return String::new();
    }
    format!(
        "{}-{:02}-{:02} {:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_numbers_are_short() {
        assert_eq!(bytes(1), "1 KB");
        assert_eq!(bytes(350_000), "342 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn a_time_reads_as_a_date() {
        let text = local_time(1_758_628_800);
        assert_eq!(text.len(), "2025-09-23 12:00".len(), "{text}");
        assert!(text.starts_with("2025-09-2"), "{text}");
    }
}
