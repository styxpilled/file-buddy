//! The file actions as a menu. The side pane's file menu and a right click on a
//! tile or a table row show the same list. The window draws the menu of a
//! right click itself, so it can fade in. Each item dispatches the action of
//! its key, so the menu shows the key beside it.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, DismissEvent, Entity, FocusHandle, Focusable,
    MouseButton, MouseDownEvent, Pixels, Point, Subscription, WeakEntity, Window, anchored,
    deferred, div, ease_out_quint, px,
};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{Icon, IconName};

use crate::search::folder::Entry;
use crate::ui::{
    CopyFile, DeleteFiles, FileBuddy, ForgetFile, MoreLikeThis, OpenFile, OpenInStudio, RenameFile,
    RevealFile, TagFiles, TrashFiles,
};

const FADE: Duration = Duration::from_millis(120);
/// The keys the open menu takes for itself, with no modifier.
const MENU_KEYS: [&str; 6] = ["up", "down", "left", "right", "enter", "escape"];

/// Every file action. `several`: the menu acts on more than one file, so the
/// items for one file are disabled.
pub(super) fn file_items(menu: PopupMenu, several: bool) -> PopupMenu {
    menu.menu_with_icon_and_disabled("Copy", icon(IconName::Copy), Box::new(CopyFile), several)
        .menu_with_icon_and_disabled(
            "Reveal",
            icon(IconName::FolderOpen),
            Box::new(RevealFile),
            several,
        )
        .menu_with_icon_and_disabled(
            "Rename",
            icon(IconName::Replace),
            Box::new(RenameFile),
            several,
        )
        .menu_with_icon_and_disabled(
            "Open",
            icon(IconName::ExternalLink),
            Box::new(OpenFile),
            several,
        )
        .menu_with_icon(
            "Open in Studio",
            icon(Lucide::Scissors),
            Box::new(OpenInStudio),
        )
        .menu_with_icon(
            "More like this",
            icon(Lucide::ScanSearch),
            Box::new(MoreLikeThis),
        )
        .menu_with_icon("Tag", icon(Lucide::Tag), Box::new(TagFiles))
        .separator()
        .menu_with_icon("Trash", icon(Lucide::Trash), Box::new(TrashFiles))
        .menu_with_icon("Delete", icon(Lucide::Shredder), Box::new(DeleteFiles))
        .menu_with_icon("Forget", icon(Lucide::DatabaseX), Box::new(ForgetFile))
}

/// The menu puts 4 px between an icon and its label, and cannot be told
/// otherwise, so the icon carries one more.
pub(super) fn icon(name: impl Into<Icon>) -> Icon {
    name.into().mr(px(1.))
}

/// Whether the menu that opens now acts on more than one file.
pub(super) fn several(view: &WeakEntity<FileBuddy>, cx: &gpui::App) -> bool {
    view.upgrade()
        .is_some_and(|view| view.read(cx).marked.len() > 1)
}

/// The menu a right click opened. The window draws it, so it can fade in.
pub(super) struct OpenMenu {
    menu: Entity<PopupMenu>,
    at: Point<Pixels>,
    /// Where the focus goes when the menu closes.
    returns: FocusHandle,
    /// A new menu fades in again, even over the last one.
    count: usize,
    _dismissed: Subscription,
}

/// A tile or a table row. A right click on a file opens the file menu.
pub(super) fn on_entry<E>(element: E, index: usize, cx: &mut Context<FileBuddy>) -> E
where
    E: InteractiveElement,
{
    element.on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            this.right_click(index, event.position, window, cx)
        }),
    )
}

/// Any key but the keys of the menu closes it. The focus goes back first, so
/// the key then does what it does without the menu.
pub(super) fn close_on_keys(cx: &mut Context<FileBuddy>) -> Subscription {
    let view = cx.weak_entity();
    cx.intercept_keystrokes(move |event, window, cx| {
        let keystroke = &event.keystroke;
        if !keystroke.modifiers.modified() && MENU_KEYS.contains(&keystroke.key.as_str()) {
            return;
        }
        view.update(cx, |this, cx| this.close_context_menu(window, cx))
            .ok();
    })
}

/// The open menu, over everything else.
pub(super) fn open_menu(this: &FileBuddy) -> Option<AnyElement> {
    let open = this.context_menu.as_ref()?;
    Some(
        deferred(
            anchored()
                .position(open.at)
                .snap_to_window_with_margin(px(8.))
                .child(div().child(open.menu.clone()).with_animation(
                    ("context-menu", open.count),
                    Animation::new(FADE).with_easing(ease_out_quint()),
                    |menu, shown| menu.opacity(shown),
                )),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

impl FileBuddy {
    /// A right click acts on the marked files when it lands on one of them.
    /// Anywhere else, it acts on the file under it alone.
    fn right_click(
        &mut self,
        index: usize,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.hit_at(index).is_some_and(|hit| self.is_marked(hit)) {
            self.marked.clear();
        }
        self.select(index, cx);
        if !matches!(self.results.get(index), Some(Entry::File(_))) {
            return;
        }
        let focus = self.focus.clone();
        let several = self.marked.len() > 1;
        self.open_context_menu(
            at,
            move |menu, _, _| file_items(menu.action_context(focus), several),
            window,
            cx,
        );
    }

    /// Close the menu with the focus back where it was, for an input the menu
    /// does not see.
    pub(super) fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.context_menu.take() {
            window.focus(&open.returns, cx);
            cx.notify();
        }
    }

    /// Show a menu at the pointer. The menu takes focus, and gives it back to
    /// its action context when it closes.
    pub(super) fn open_context_menu(
        &mut self,
        at: Point<Pixels>,
        build: impl FnOnce(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let returns = match &self.context_menu {
            Some(open) => open.returns.clone(),
            None => window.focused(cx).unwrap_or_else(|| self.focus.clone()),
        };
        let menu = PopupMenu::build(window, cx, build);
        // After the mouse down is done, or the window takes the focus back.
        let focus = menu.focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        let dismissed = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
            this.context_menu = None;
            cx.notify();
        });
        let count = self.context_menu.as_ref().map_or(0, |open| open.count + 1);
        self.context_menu = Some(OpenMenu {
            menu,
            at,
            returns,
            count,
            _dismissed: dismissed,
        });
        cx.notify();
    }
}
