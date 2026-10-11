//! Back and forward, as in a browser. A step saves the state the user leaves:
//! the page, the folder, the query, the Review group, the selection and the
//! scroll position. The chips, the sort and the view are not part of a step.

use std::collections::VecDeque;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    Context, DispatchPhase, MouseButton, MouseDownEvent, NavigationDirection, Pixels, Point,
    ScrollStrategy, Window, canvas,
};

use super::visual::Like;
use super::{Entry, FileBuddy, Group, Mode, Page, Settings};

const LIMIT: usize = 100;

/// The steps on each side of where the user is.
pub(super) struct History<T> {
    back: VecDeque<T>,
    forward: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            back: VecDeque::new(),
            forward: Vec::new(),
        }
    }
}

impl<T> History<T> {
    /// The user leaves `left` for a new place. The forward steps go.
    pub(super) fn push(&mut self, left: T) {
        if self.back.len() == LIMIT {
            self.back.pop_front();
        }
        self.back.push_back(left);
        self.forward.clear();
    }

    /// The last step back that `usable` accepts. The steps it passes over are
    /// dropped. `current` becomes the first step forward.
    pub(super) fn back(&mut self, current: T, usable: impl Fn(&T) -> bool) -> Option<T> {
        while let Some(step) = self.back.pop_back() {
            if usable(&step) {
                self.forward.push(current);
                return Some(step);
            }
        }
        None
    }

    pub(super) fn forward(&mut self, current: T, usable: impl Fn(&T) -> bool) -> Option<T> {
        while let Some(step) = self.forward.pop() {
            if usable(&step) {
                self.back.push_back(current);
                return Some(step);
            }
        }
        None
    }
}

pub(super) struct Step {
    page: Page,
    folder: Option<PathBuf>,
    dropped: bool,
    query: String,
    likes: Vec<Like>,
    group: Option<Group>,
    selected: Option<Entry>,
    scroll: Point<Pixels>,
}

impl Step {
    /// A folder that no longer exists is skipped.
    fn usable(&self) -> bool {
        self.page != Page::Folders || self.folder.as_ref().is_none_or(|dir| dir.is_dir())
    }
}

/// Where the selection and the scroll go when the results of a step arrive.
pub(super) struct Landing {
    selected: Option<Entry>,
    scroll: Point<Pixels>,
}

#[derive(Default)]
pub(super) struct Travel {
    steps: History<Step>,
    /// Set until the results of the step arrive.
    landing: Option<Landing>,
    /// The selected file of a step was not in the text matches. It can come
    /// with the visual matches.
    late: Option<Entry>,
}

impl FileBuddy {
    fn step(&self, cx: &Context<Self>) -> Step {
        Step {
            page: self.page(),
            folder: self.folder.clone(),
            dropped: self.dropped,
            query: self.input.read(cx).value().to_string(),
            likes: self.visual.likes_without_results(),
            group: self.review().map(|review| review.group),
            selected: self.results.get(self.selected).cloned(),
            scroll: self.scroll.offset(),
        }
    }

    /// Save the state the user leaves.
    pub(super) fn record_step(&mut self, cx: &Context<Self>) {
        let step = self.step(cx);
        self.travel.steps.push(step);
        self.travel.landing = None;
        self.travel.late = None;
    }

    /// The mouse back and forward buttons, `alt-left` and `alt-right`. Returns
    /// false when there is no step to go to.
    pub(super) fn go_history(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let current = self.step(cx);
        let step = if forward {
            self.travel.steps.forward(current, Step::usable)
        } else {
            self.travel.steps.back(current, Step::usable)
        };
        let Some(step) = step else {
            return false;
        };
        self.end_autoscroll(cx);
        self.folder = step.folder;
        self.dropped = step.dropped;
        self.visual.set_likes(step.likes);
        self.input
            .update(cx, |input, cx| input.set_value(step.query, window, cx));
        self.travel.landing = Some(Landing {
            selected: step.selected,
            scroll: step.scroll,
        });
        self.travel.late = None;
        self.enter_page(step.page, step.group, window, cx);
        true
    }

    /// A page with the state that page has. Its results are new.
    pub(super) fn enter_page(
        &mut self,
        page: Page,
        group: Option<Group>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tagger = None;
        self.renamer = None;
        window.focus(&self.focus, cx);
        if page != Page::Studio {
            self.close_studio_clip();
        }
        match page {
            Page::Browse => {
                self.mode = Mode::Browse;
                self.run_search(cx);
            }
            Page::Folders => {
                self.mode = Mode::Folders;
                self.run_search(cx);
            }
            Page::Review => self.open_group(group, window, cx),
            Page::Settings => {
                self.mode = Mode::Settings(Box::new(Settings::new(&self.config, window, cx)));
                self.listing = false;
                // Nothing plays behind the form, and no key acts on a file that
                // cannot be seen.
                self.show_results(Vec::new(), cx);
            }
            Page::Studio => {
                self.mode = Mode::Studio;
                self.listing = false;
                self.show_results(Vec::new(), cx);
                self.open_studio_clip();
            }
        }
    }

    /// New results arrived. If they are the results of a step, the selection
    /// and the scroll go back to where they were.
    pub(super) fn land(&mut self) {
        self.travel.late = None;
        let Some(landing) = self.travel.landing.take() else {
            return;
        };
        let found = landing
            .selected
            .as_ref()
            .and_then(|entry| self.position_of(entry));
        match found {
            Some(at) => self.selected = at,
            None => self.travel.late = landing.selected,
        }
        self.scroll.set_offset(landing.scroll);
        self.scroll
            .scroll_to_item(self.selected / self.columns.max(1), ScrollStrategy::Nearest);
    }

    /// The visual matches merged in. Returns whether the selected file of a
    /// step came with them.
    pub(super) fn land_late(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(at) = self
            .travel
            .late
            .take()
            .and_then(|entry| self.position_of(&entry))
        else {
            return false;
        };
        self.select(at, cx);
        true
    }
}

/// The mouse back and forward buttons, over the whole window. A listener on an
/// element misses the parts of the header that block the mouse to what is
/// behind them.
pub(super) fn mouse_buttons(cx: &mut Context<FileBuddy>) -> impl IntoElement {
    let view = cx.entity();
    canvas(
        |_, _, _| (),
        move |_, (), window, _| {
            let view = view.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture
                    && let MouseButton::Navigate(direction) = event.button
                {
                    // The menu never sees this button, as it stops here.
                    view.update(cx, |this, cx| {
                        this.close_context_menu(window, cx);
                        this.go_history(direction == NavigationDirection::Forward, window, cx);
                    });
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn back(history: &mut History<u32>, current: u32) -> Option<u32> {
        history.back(current, |_| true)
    }

    fn forward(history: &mut History<u32>, current: u32) -> Option<u32> {
        history.forward(current, |_| true)
    }

    #[test]
    fn back_and_forward_return_the_steps() {
        let mut history = History::default();
        history.push(1);
        history.push(2);
        assert_eq!(back(&mut history, 3), Some(2));
        assert_eq!(back(&mut history, 2), Some(1));
        assert_eq!(back(&mut history, 1), None);
        assert_eq!(forward(&mut history, 1), Some(2));
        assert_eq!(forward(&mut history, 2), Some(3));
        assert_eq!(forward(&mut history, 3), None);
        assert_eq!(back(&mut history, 3), Some(2));
    }

    #[test]
    fn a_new_step_after_going_back_drops_the_forward_steps() {
        let mut history = History::default();
        history.push(1);
        history.push(2);
        assert_eq!(back(&mut history, 3), Some(2));
        history.push(2);
        assert_eq!(forward(&mut history, 4), None);
        assert_eq!(back(&mut history, 4), Some(2));
        assert_eq!(back(&mut history, 2), Some(1));
    }

    #[test]
    fn at_most_the_limit_of_steps_is_kept() {
        let mut history = History::default();
        for step in 0..LIMIT as u32 + 5 {
            history.push(step);
        }
        let mut current = 1000;
        let mut count = 0;
        while let Some(step) = back(&mut history, current) {
            current = step;
            count += 1;
        }
        assert_eq!(count, LIMIT);
        assert_eq!(current, 5);
    }

    #[test]
    fn a_step_that_is_not_usable_is_skipped_and_dropped() {
        let mut history = History::default();
        history.push(1);
        history.push(2);
        history.push(3);
        assert_eq!(history.back(4, |step| *step != 3), Some(2));
        assert_eq!(forward(&mut history, 2), Some(4));
        assert_eq!(back(&mut history, 4), Some(2));
        // Nothing usable: the user stays, and nothing moves forward.
        assert_eq!(history.back(2, |_| false), None);
        assert_eq!(forward(&mut history, 2), Some(4));
    }

    #[test]
    fn a_missing_folder_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let step = |page, folder: Option<PathBuf>| Step {
            page,
            folder,
            dropped: false,
            query: String::new(),
            likes: Vec::new(),
            group: None,
            selected: None,
            scroll: Point::default(),
        };
        let gone = dir.path().join("gone");
        assert!(step(Page::Folders, Some(dir.path().to_path_buf())).usable());
        assert!(step(Page::Folders, None).usable());
        assert!(!step(Page::Folders, Some(gone.clone())).usable());
        // Browse does not show the folder.
        assert!(step(Page::Browse, Some(gone)).usable());
    }
}
