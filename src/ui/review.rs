//! The Review page: the files that need a person, in four groups. Its tabs are
//! at the left of the bar over the results, and the chips limit every group. The Poor names group is the
//! rename walk: the clip plays, and a saved name moves on to the next file.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Window};
use gpui_kit::component::{Sizable, h_flex};

use crate::db::review::{self as groups, Group, Item};
use crate::search::folder::Entry;
use crate::search::{Hit, Signal};
use crate::ui::{FileBuddy, Mode, toggle};

pub(super) struct Review {
    pub group: Group,
    /// Files in each group, in the order of `Group::ALL`.
    counts: [usize; 4],
}

impl FileBuddy {
    pub(super) fn review(&self) -> Option<&Review> {
        match &self.mode {
            Mode::Review(review) => Some(review),
            _ => None,
        }
    }

    pub(super) fn walking(&self) -> bool {
        self.review()
            .is_some_and(|review| review.group == Group::PoorNames)
    }

    /// A tab of Review, from a click or `alt-1` to `alt-4`. It opens Review
    /// when it is closed.
    pub(super) fn show_group(&mut self, group: Group, window: &mut Window, cx: &mut Context<Self>) {
        if self.review().is_none_or(|review| review.group != group) {
            self.record_step(cx);
        }
        self.open_group(Some(group), window, cx);
    }

    /// With no group, Review opens on the first that holds a file.
    pub(super) fn open_group(
        &mut self,
        group: Option<Group>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = self.review_items(cx);
        let counts = items.each_ref().map(Vec::len);
        let group = group
            .or_else(|| {
                Group::ALL
                    .into_iter()
                    .find(|group| counts[*group as usize] > 0)
            })
            .unwrap_or(Group::Duplicates);
        self.mode = Mode::Review(Review { group, counts });
        self.listing = false;
        self.tagger = None;
        self.renamer = None;
        self.player.step = 1;
        window.focus(&self.focus, cx);

        let results = entries(items, group);
        let walk = self.walking() && !results.is_empty();
        if walk {
            // The clip on screen may already be the first file, and the
            // preview keeps that one as it is.
            self.player.paused = false;
            if let Some(Entry::File(first)) = results.first()
                && let Some(video) = self.player.video_of(&first.file.hash)
            {
                video.set_paused(false);
            }
        }
        self.show_results(results, cx);
        if walk {
            self.open_renamer(window, cx);
        }
    }

    /// The group again, after a rename changed it.
    pub(super) fn reload_review(&mut self, cx: &mut Context<Self>) {
        let Some(group) = self.review().map(|review| review.group) else {
            return;
        };
        let items = self.review_items(cx);
        self.mode = Mode::Review(Review {
            group,
            counts: items.each_ref().map(Vec::len),
        });
        self.update_results(entries(items, group), cx);
    }

    /// Count the groups again. The walk keeps its results, because a named
    /// file must not leave the list under the user.
    pub(super) fn count_groups(&mut self, cx: &mut Context<Self>) {
        let counts = self.review_items(cx).each_ref().map(Vec::len);
        if let Mode::Review(review) = &mut self.mode {
            review.counts = counts;
        }
    }

    /// The files of every group of the kinds the chips let through, in the
    /// order of `Group::ALL`.
    fn review_items(&mut self, cx: &mut Context<Self>) -> [Vec<Item>; 4] {
        let mut failure = None;
        let items = Group::ALL.map(|group| {
            let mut items = groups::items(self.db.conn(), group).unwrap_or_else(|e| {
                log::error!("reading {} failed: {e:#}", group.label());
                failure = Some(format!("reading {} failed: {e}", group.label()));
                Vec::new()
            });
            items.retain(|item| self.kinds.contains(&item.file.kind));
            items
        });
        if let Some(text) = failure {
            self.flash(text, true, cx);
        }
        items
    }
}

/// The files of one group, as results. The second line of a tile says why the
/// file is here, so it holds the note and not a match.
fn entries(items: [Vec<Item>; 4], group: Group) -> Vec<Entry> {
    let items = items.into_iter().nth(group as usize).unwrap_or_default();
    items
        .into_iter()
        .map(|item| {
            Entry::File(Hit {
                file: item.file,
                matched: item.note,
                score: 0.0,
                signal: Signal::Filename,
                at_ms: None,
                named: false,
            })
        })
        .collect()
}

pub fn tabs(review: &Review, cx: &mut Context<FileBuddy>) -> AnyElement {
    h_flex()
        .gap_1()
        .children(Group::ALL.into_iter().zip(review.counts).enumerate().map(
            |(at, (group, count))| {
                toggle(
                    group.label(),
                    format!("{}  {count}", group.label()),
                    review.group == group,
                )
                .xsmall()
                .tooltip(format!("Alt+{}", at + 1))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.show_group(group, window, cx)),
                )
            },
        ))
        .into_any_element()
}
