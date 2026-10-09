//! The Settings page. It edits the fields of `file-buddy.toml`: the folders to
//! index, the ignore globs, the export folder, the search weights, the paused
//! stages and the keys.
//! A switch that pauses a stage is written at once. Nothing else is written
//! until Save, and Save writes nothing that `Config::edited` refuses or that
//! gives one key to two actions.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Keystroke, PathPromptOptions, ScrollHandle,
    Subscription, Window, div, px,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};

use crate::config::{Config, Paused};
use crate::index::jobs::Stage;
use crate::paths;
use crate::ui::keys::{self, ACTIONS};
use crate::ui::{FileBuddy, Mode};

const PAGE_WIDTH: f32 = 720.;
const WEIGHT_WIDTH: f32 = 120.;
const WEIGHT_NAMES: [&str; 5] = ["Tags", "Filename", "Speech", "Screen text", "Visual"];
const KEY_LIST_HEIGHT: f32 = 320.;
const KEY_NAME_WIDTH: f32 = 180.;

pub struct Settings {
    roots: Vec<PathBuf>,
    ignore: Entity<TextareaState>,
    export_folder: Option<PathBuf>,
    /// In column order, as `Config::edited` takes them.
    weights: [Entity<InputState>; 5],
    /// The `[keys]` table as edited: only the actions that differ.
    keys: BTreeMap<String, Vec<String>>,
    /// The row of `ACTIONS` that the keys act on.
    key_row: usize,
    key_focus: FocusHandle,
    key_scroll: ScrollHandle,
    /// The next key pressed goes to the row.
    capturing: bool,
}

impl Settings {
    pub fn new(config: &Config, window: &mut Window, cx: &mut App) -> Self {
        let weights = config.weights;
        let values = [
            weights.tags,
            weights.filename,
            weights.speech,
            weights.ocr,
            weights.visual,
        ];
        Self {
            roots: config.roots.clone(),
            ignore: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(4, 12)
                    .default_value(config.ignore.join("\n"))
            }),
            export_folder: config.export_folder.clone(),
            weights: values.map(|value| {
                cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()))
            }),
            keys: config.keys.clone(),
            key_row: 0,
            key_focus: cx.focus_handle(),
            key_scroll: ScrollHandle::new(),
            capturing: false,
        }
    }

    /// Give a row these keys. A row with its default keys leaves the table.
    fn set_keys(&mut self, name: &str, keys: Vec<String>) {
        let action = ACTIONS.iter().find(|action| action.name == name);
        if action.is_some_and(|action| keys == keys::keys_of(action, &BTreeMap::new())) {
            self.keys.remove(name);
        } else {
            self.keys.insert(name.to_string(), keys);
        }
    }

    /// The form as `Config::edited` checks it.
    fn edited(&self, paused: Paused, cx: &App) -> anyhow::Result<Config> {
        let ignore = self.ignore.read(cx).value().to_string();
        let weights = self
            .weights
            .each_ref()
            .map(|input| input.read(cx).value().to_string());
        Config::edited(
            self.roots.clone(),
            &ignore,
            self.export_folder.clone(),
            weights.each_ref().map(String::as_str),
            paused,
            self.keys.clone(),
        )
    }
}

impl FileBuddy {
    pub(super) fn settings(&self) -> Option<&Settings> {
        match &self.mode {
            Mode::Settings(settings) => Some(settings.as_ref()),
            _ => None,
        }
    }

    fn settings_mut(&mut self) -> Option<&mut Settings> {
        match &mut self.mode {
            Mode::Settings(settings) => Some(settings.as_mut()),
            _ => None,
        }
    }

    /// Check the form, write the file, and use it. New weights reach the next
    /// search; new folders or globs start a scan.
    pub(super) fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Some(settings) = self.settings() else {
            return;
        };
        let config = match settings.edited(self.config.paused, cx) {
            Ok(config) => config,
            Err(e) => {
                self.flash(e.to_string(), true, cx);
                return;
            }
        };
        if let Some(conflict) = keys::resolve(&config.keys).conflicts.first() {
            self.flash(conflict.clone(), true, cx);
            return;
        }
        if let Err(e) = paths::config_path().and_then(|path| config.write(&path)) {
            log::error!("saving the settings failed: {e:#}");
            self.flash(format!("saving failed: {e}"), true, cx);
            return;
        }
        let rescan = config.roots != self.config.roots || config.ignore != self.config.ignore;
        if config.keys != self.config.keys {
            self.key_problems = keys::apply(&config.keys, cx);
        }
        self.config = config;
        if rescan {
            self.flash("saved, scanning the folders", false, cx);
            self.start_watching();
            self.rescan(cx);
        } else {
            self.flash("saved", false, cx);
        }
    }

    fn revert_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings().is_some() {
            self.mode = Mode::Settings(Box::new(Settings::new(&self.config, window, cx)));
            cx.notify();
        }
    }

    fn add_roots(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            this.update(cx, |this, cx| {
                let Some(settings) = this.settings_mut() else {
                    return;
                };
                for folder in folders {
                    if !settings.roots.contains(&folder) {
                        settings.roots.push(folder);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn choose_export_folder(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            this.update(cx, |this, cx| {
                if let Some(settings) = this.settings_mut() {
                    settings.export_folder = folders.into_iter().next();
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn clear_export_folder(&mut self, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut() {
            settings.export_folder = None;
            cx.notify();
        }
    }

    /// Up or down on the page, outside the fields, goes into the keys.
    pub(super) fn focus_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(settings) = self.settings() else {
            return false;
        };
        window.focus(&settings.key_focus, cx);
        cx.notify();
        true
    }

    fn select_key_row(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut() {
            settings.key_row = at;
            settings.capturing = false;
        }
        self.focus_keys(window, cx);
    }

    pub(super) fn move_key_row(&mut self, down: bool, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut() {
            settings.key_row = if down {
                (settings.key_row + 1).min(ACTIONS.len() - 1)
            } else {
                settings.key_row.saturating_sub(1)
            };
            settings.key_scroll.scroll_to_item(settings.key_row);
            cx.notify();
        }
    }

    /// Enter. The next key pressed goes to the row, in `capture_key`.
    pub(super) fn start_capture(&mut self, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut() {
            settings.capturing = true;
            cx.notify();
        }
    }

    /// `delete`. The last key of the row goes.
    pub(super) fn remove_key(&mut self, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut() {
            let action = &ACTIONS[settings.key_row];
            let mut keys = keys::keys_of(action, &settings.keys);
            keys.pop();
            settings.set_keys(action.name, keys);
            cx.notify();
        }
    }

    /// `r`. The row goes back to its default keys.
    pub(super) fn reset_keys(&mut self, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut() {
            settings.keys.remove(ACTIONS[settings.key_row].name);
            cx.notify();
        }
    }

    /// The key after Enter in the Keys section. It must not also fire the
    /// action it is bound to, so it is taken before the keymap sees it.
    fn capture_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(settings) = self.settings_mut().filter(|settings| settings.capturing) else {
            return false;
        };
        // A modifier on its own is the start of the key, not the key.
        if matches!(
            keystroke.key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function"
        ) {
            return false;
        }
        settings.capturing = false;
        if keystroke.key != "escape"
            && let Some(key) = keys::normal(&keystroke.unparse())
        {
            let action = &ACTIONS[settings.key_row];
            let mut keys = keys::keys_of(action, &settings.keys);
            if !keys.contains(&key) {
                keys.push(key);
            }
            settings.set_keys(action.name, keys);
        }
        cx.notify();
        true
    }

    fn remove_root(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut()
            && at < settings.roots.len()
        {
            settings.roots.remove(at);
            cx.notify();
        }
    }
}

pub fn page(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(settings) = this.settings() else {
        return div().into_any_element();
    };

    div()
        .size_full()
        .overflow_y_scrollbar()
        .child(
            v_flex()
                .w(px(PAGE_WIDTH))
                .gap_6()
                .p_6()
                .child(folders(settings, cx))
                .child(
                    section(
                        "Ignore",
                        "One glob per line. A path that matches is not indexed.",
                        cx,
                    )
                    .child(Textarea::new(&settings.ignore)),
                )
                .child(export_folder(settings, cx))
                .child(weights(settings, cx))
                .child(stages(this.config.paused, cx))
                .child(key_list(settings, cx))
                .child(footer(cx)),
        )
        .into_any_element()
}

fn section(title: &'static str, about: &'static str, cx: &App) -> gpui::Div {
    v_flex()
        .gap_2()
        .child(div().text_base().child(title))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(about),
        )
}

fn folders(settings: &Settings, cx: &mut Context<FileBuddy>) -> AnyElement {
    section(
        "Folders",
        "Every file under these folders is indexed. A change starts a scan when you save.",
        cx,
    )
    .children(settings.roots.iter().enumerate().map(|(at, root)| {
        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .truncate()
                    .child(root.display().to_string()),
            )
            .child(
                Button::new(("remove-root", at))
                    .small()
                    .label("Remove")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_root(at, cx))),
            )
    }))
    .child(
        h_flex().child(
            Button::new("add-root")
                .small()
                .label("Add folder")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.add_roots(cx))),
        ),
    )
    .into_any_element()
}

fn export_folder(settings: &Settings, cx: &mut Context<FileBuddy>) -> AnyElement {
    let (text, set) = match &settings.export_folder {
        Some(folder) => (folder.display().to_string(), true),
        None => (
            "None. The Studio does not export without one.".to_string(),
            false,
        ),
    };
    section(
        "Export folder",
        "The Studio writes its exports here. Add it to the folders above to index them.",
        cx,
    )
    .child(
        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .truncate()
                    .when(!set, |text| text.text_color(cx.theme().muted_foreground))
                    .child(text),
            )
            .child(
                Button::new("choose-export-folder")
                    .small()
                    .label("Choose folder")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.choose_export_folder(cx))),
            )
            .when(set, |row| {
                row.child(
                    Button::new("clear-export-folder")
                        .small()
                        .label("Clear")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.clear_export_folder(cx))),
                )
            }),
    )
    .into_any_element()
}

fn weights(settings: &Settings, cx: &App) -> AnyElement {
    section(
        "Search weights",
        "The sort orders the results. These only pick the 500 best text matches when more match. Visual above 0 adds the files whose frames look like the query, after the text matches.",
        cx,
    )
    .children(
        settings
            .weights
            .iter()
            .zip(WEIGHT_NAMES)
            .map(|(input, name)| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(WEIGHT_WIDTH)).text_sm().child(name))
                    .child(div().w(px(WEIGHT_WIDTH)).child(Input::new(input).small()))
            }),
    )
    .into_any_element()
}

fn stages(paused: Paused, cx: &mut Context<FileBuddy>) -> AnyElement {
    section(
        "Paused stages",
        "A paused stage doesn't start any new jobs.",
        cx,
    )
    .children(
        [
            (Stage::Ocr, paused.ocr, "Screen text (OCR)"),
            (Stage::Clip, paused.clip, "Visual/semantic (CLIP)"),
            (Stage::Whisper, paused.whisper, "Speech (Whisper)"),
        ]
        .map(|(stage, checked, label)| {
            Switch::new(stage.as_str())
                .small()
                .checked(checked)
                .label(label)
                .cursor_pointer()
                .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                    this.pause_stage(stage, *checked, cx)
                }))
        }),
    )
    .into_any_element()
}

/// Takes keys for the Keys section before the keymap does. It acts only while
/// a row captures.
pub(super) fn capture_keys(cx: &mut Context<FileBuddy>) -> Subscription {
    let view = cx.weak_entity();
    cx.intercept_keystrokes(move |event, _, cx| {
        let captured = view
            .update(cx, |this, cx| this.capture_key(&event.keystroke, cx))
            .unwrap_or(false);
        if captured {
            cx.stop_propagation();
        }
    })
}

/// Each action with its keys. A row that differs from the default is marked,
/// and what cannot apply is named under the list.
fn key_list(settings: &Settings, cx: &mut Context<FileBuddy>) -> AnyElement {
    let problems = keys::resolve(&settings.keys).problems();
    let rows = ACTIONS.iter().enumerate().map(|(at, action)| {
        let current = at == settings.key_row;
        let capturing = current && settings.capturing;
        let keys = keys::keys_of(action, &settings.keys);
        let text = if capturing {
            "press a key".to_string()
        } else if keys.is_empty() {
            "none".to_string()
        } else {
            keys.join(", ")
        };
        h_flex()
            .id(("key-row", at))
            .gap_2()
            .px_1()
            .rounded(px(3.))
            .text_sm()
            .cursor_pointer()
            .when(current, |row| row.bg(cx.theme().primary.opacity(0.18)))
            .on_click(cx.listener(move |this, _, window, cx| this.select_key_row(at, window, cx)))
            .child(div().w(px(KEY_NAME_WIDTH)).child(action.name))
            .child(
                div()
                    .flex_1()
                    .when(keys.is_empty() || capturing, |text| {
                        text.text_color(cx.theme().muted_foreground)
                    })
                    .child(text),
            )
            .when(settings.keys.contains_key(action.name), |row| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child("changed"),
                )
            })
    });

    section(
        "Keys",
        "Up and down pick an action. Enter adds the next key you press, delete removes the last key, r resets the row to its default, and escape cancels. A key with ctrl or alt, or an F key, works in the text boxes too.",
        cx,
    )
    .child(
        // GPUI scrolls every scroll area under the pointer, so the page would
        // move with the list. The wheel stops here, after the list took it.
        div()
            .id("keys-frame")
            .relative()
            .h(px(KEY_LIST_HEIGHT))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("keys")
                    .track_focus(&settings.key_focus)
                    .key_context("Keys")
                    .track_scroll(&settings.key_scroll)
                    .size_full()
                    .overflow_y_scroll()
                    .p_1()
                    .rounded(px(4.))
                    .bg(cx.theme().secondary)
                    .children(rows),
            )
            .vertical_scrollbar(&settings.key_scroll),
    )
    .children(problems.into_iter().map(|problem| {
        div()
            .text_sm()
            .text_color(cx.theme().danger)
            .child(problem)
    }))
    .into_any_element()
}

fn footer(cx: &mut Context<FileBuddy>) -> AnyElement {
    let file = paths::config_path()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    h_flex()
        .gap_2()
        .items_center()
        .child(
            Button::new("save-settings")
                .small()
                .primary()
                .label("Save")
                .tooltip("Ctrl+S")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
        )
        .child(
            Button::new("revert-settings")
                .small()
                .label("Revert")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.revert_settings(window, cx))),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(file),
        )
        .into_any_element()
}
