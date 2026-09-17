//! The Settings page. It edits the fields of `file-buddy.toml`: the folders to
//! index, the ignore globs and the search weights. Nothing is written until
//! Save, and Save writes nothing that `Config::edited` refuses.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, Window, div, px};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};

use crate::config::Config;
use crate::paths;
use crate::ui::{FileBuddy, with_pointer};

const PAGE_WIDTH: f32 = 720.;
const WEIGHT_WIDTH: f32 = 120.;
const WEIGHT_NAMES: [&str; 4] = ["Tags", "Filename", "Speech", "Screen text"];

pub struct Settings {
    pub roots: Vec<PathBuf>,
    pub ignore: Entity<TextareaState>,
    /// In column order, as `Config::edited` takes them.
    pub weights: [Entity<InputState>; 4],
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
    pub fn edited(&self, cx: &App) -> anyhow::Result<Config> {
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

pub fn page(this: &FileBuddy, cx: &mut Context<FileBuddy>) -> AnyElement {
    let Some(settings) = &this.settings else {
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
            .child(with_pointer(
                Button::new(("remove-root", at))
                    .small()
                    .label("Remove")
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_root(at, cx))),
                true,
            ))
    }))
    .child(
        h_flex().child(with_pointer(
            Button::new("add-root")
                .small()
                .label("Add folder")
                .on_click(cx.listener(|this, _, _, cx| this.add_roots(cx))),
            true,
        )),
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
        .child(with_pointer(
            Button::new("save-settings")
                .small()
                .primary()
                .label("Save")
                .tooltip("Ctrl+S")
                .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
            true,
        ))
        .child(with_pointer(
            Button::new("revert-settings")
                .small()
                .label("Revert")
                .on_click(cx.listener(|this, _, window, cx| this.revert_settings(window, cx))),
            true,
        ))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(file),
        )
        .into_any_element()
}
