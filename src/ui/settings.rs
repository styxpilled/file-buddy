//! The Settings page. It edits the fields of `file-buddy.toml`: the folders to
//! index, the ignore globs and the search weights. Nothing is written until
//! Save, and Save writes nothing that `Config::edited` refuses.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, PathPromptOptions, Window, div, px};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};

use crate::config::Config;
use crate::paths;
use crate::ui::{FileBuddy, Mode};

const PAGE_WIDTH: f32 = 720.;
const WEIGHT_WIDTH: f32 = 120.;
const WEIGHT_NAMES: [&str; 4] = ["Tags", "Filename", "Speech", "Screen text"];

pub struct Settings {
    roots: Vec<PathBuf>,
    ignore: Entity<TextareaState>,
    /// In column order, as `Config::edited` takes them.
    weights: [Entity<InputState>; 4],
}

impl Settings {
    pub fn new(config: &Config, window: &mut Window, cx: &mut App) -> Self {
        let weights = config.weights;
        let values = [weights.tags, weights.filename, weights.speech, weights.ocr];
        Self {
            roots: config.roots.clone(),
            ignore: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(4, 12)
                    .default_value(config.ignore.join("\n"))
            }),
            weights: values.map(|value| {
                cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()))
            }),
        }
    }

    /// The form as `Config::edited` checks it.
    fn edited(&self, cx: &App) -> anyhow::Result<Config> {
        let ignore = self.ignore.read(cx).value().to_string();
        let weights = self
            .weights
            .each_ref()
            .map(|input| input.read(cx).value().to_string());
        Config::edited(
            self.roots.clone(),
            &ignore,
            weights.each_ref().map(String::as_str),
        )
    }
}

impl FileBuddy {
    pub(super) fn settings(&self) -> Option<&Settings> {
        match &self.mode {
            Mode::Settings(settings) => Some(settings),
            _ => None,
        }
    }

    fn settings_mut(&mut self) -> Option<&mut Settings> {
        match &mut self.mode {
            Mode::Settings(settings) => Some(settings),
            _ => None,
        }
    }

    /// Check the form, write the file, and use it. New weights reach the next
    /// search; new folders or globs start a scan.
    pub(super) fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Some(settings) = self.settings() else {
            return;
        };
        let config = match settings.edited(cx) {
            Ok(config) => config,
            Err(e) => {
                self.flash(e.to_string(), true, cx);
                return;
            }
        };
        if let Err(e) = paths::config_path().and_then(|path| config.write(&path)) {
            log::error!("saving the settings failed: {e:#}");
            self.flash(format!("saving failed: {e}"), true, cx);
            return;
        }
        let rescan = config.roots != self.config.roots || config.ignore != self.config.ignore;
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
            self.mode = Mode::Settings(Settings::new(&self.config, window, cx));
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
        .id("settings")
        .size_full()
        .overflow_y_scroll()
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
                .child(weights(settings, cx))
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

fn weights(settings: &Settings, cx: &App) -> AnyElement {
    section(
        "Search weights",
        "How much a match in each kind of text counts. A higher number ranks it above the others.",
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
