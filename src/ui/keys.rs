//! The window keys. The defaults, the changes in the `[keys]` table of the
//! configuration, and the rules that join them. The context of a key comes from
//! the key: a key with `ctrl` or `alt`, or an F key, works everywhere, and any
//! other key works only outside the text inputs.

use std::collections::{BTreeMap, HashMap};

use gpui::{App, Global, KeyBinding, Keystroke};

use super::*;

/// Where an action works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    Window,
    /// The rows of the Speech and Text tabs, while they have the cursor.
    Rows,
}

pub(super) struct Action {
    pub(super) name: &'static str,
    scope: Scope,
    pub(super) defaults: &'static [&'static str],
    bind: fn(&str, Option<&str>) -> KeyBinding,
}

macro_rules! action {
    ($name:literal, $scope:ident, $action:ident, [$($key:literal),*]) => {
        Action {
            name: $name,
            scope: Scope::$scope,
            defaults: &[$($key),*],
            bind: |key, context| KeyBinding::new(key, $action, context),
        }
    };
}

/// Every action whose keys can change, with its default keys.
pub(super) const ACTIONS: &[Action] = &[
    action!("focus_query", Window, FocusQuery, ["f"]),
    action!("toggle_muted", Window, ToggleMuted, ["m"]),
    action!("toggle_paused", Window, TogglePaused, ["space"]),
    action!("toggle_autoplay", Window, ToggleAutoplay, ["ctrl-space"]),
    action!("volume_up", Window, VolumeUp, ["ctrl-up"]),
    action!("volume_down", Window, VolumeDown, ["ctrl-down"]),
    action!("skip_back", Window, SkipBack, ["shift-left"]),
    action!("skip_forward", Window, SkipForward, ["shift-right"]),
    action!("cycle_sort", Window, CycleSort, ["s"]),
    action!("toggle_table", Window, ToggleTable, ["v"]),
    action!("more_like_this", Window, MoreLikeThis, ["a"]),
    action!("reveal_file", Window, RevealFile, ["ctrl-e"]),
    action!("open_file", Window, OpenFile, ["ctrl-o"]),
    action!("tag_files", Window, TagFiles, ["t"]),
    action!("toggle_mark", Window, ToggleMark, ["x"]),
    action!("rename_file", Window, RenameFile, ["r", "ctrl-r", "f2"]),
    action!("trash_files", Window, TrashFiles, ["delete"]),
    action!("delete_files", Window, DeleteFiles, ["shift-delete"]),
    action!("review_duplicates", Window, ReviewDuplicates, ["alt-1"]),
    action!("review_same_names", Window, ReviewSameNames, ["alt-2"]),
    action!("review_long", Window, ReviewLong, ["alt-3"]),
    action!("review_failed", Window, ReviewFailed, ["alt-4"]),
    action!("review_poor_names", Window, ReviewPoorNames, ["alt-5"]),
    action!("rescan", Window, Rescan, ["f5"]),
    action!("cycle_detail", Window, CycleDetail, ["i"]),
    action!("edit_rows", Window, EditRows, ["e"]),
    action!("delete_row", Rows, DeleteRow, ["delete"]),
    action!("add_row", Rows, AddRow, ["l"]),
];

/// The keys that cannot change, with the action that holds each.
const FIXED: &[(&str, &str)] = &[
    ("left", "select_left"),
    ("right", "select_right"),
    ("up", "select_up"),
    ("down", "select_down"),
    ("up", "row_up"),
    ("down", "row_down"),
    ("enter", "activate"),
    ("enter", "edit_row"),
    ("escape", "dismiss"),
    ("backspace", "go_up"),
    ("ctrl-c", "copy_file"),
    ("ctrl-s", "save_settings"),
    ("ctrl-1", "show_browse"),
    ("ctrl-2", "show_folders"),
    ("ctrl-3", "show_review"),
    ("ctrl-4", "show_settings"),
    ("alt-up", "previous_suggestion"),
    ("alt-down", "next_suggestion"),
    ("alt-left", "history_back"),
    ("alt-right", "history_forward"),
];

fn fixed_bindings() -> Vec<KeyBinding> {
    let grid = Some("FileBuddy && !Input");
    let rows = Some("Rows && !Input");
    let window = Some("FileBuddy");
    let keys = Some("Keys");
    vec![
        KeyBinding::new("left", SelectLeft, grid),
        KeyBinding::new("right", SelectRight, grid),
        KeyBinding::new("up", SelectUp, grid),
        KeyBinding::new("down", SelectDown, grid),
        KeyBinding::new("enter", Activate, grid),
        KeyBinding::new("backspace", GoUp, grid),
        // `input::Escape` reaches the view only while an input has focus, so
        // escape needs a binding of its own for the rest of the time.
        KeyBinding::new("escape", Dismiss, grid),
        KeyBinding::new("ctrl-c", CopyFile, window),
        KeyBinding::new("ctrl-s", SaveSettings, window),
        KeyBinding::new("ctrl-1", ShowBrowse, window),
        KeyBinding::new("ctrl-2", ShowFolders, window),
        KeyBinding::new("ctrl-3", ShowReview, window),
        KeyBinding::new("ctrl-4", ShowSettings, window),
        // Plain arrows belong to the text in the tag input, so the suggestion
        // list takes the ones the input leaves alone.
        KeyBinding::new("alt-down", NextSuggestion, window),
        KeyBinding::new("alt-up", PreviousSuggestion, window),
        KeyBinding::new("alt-left", HistoryBack, window),
        KeyBinding::new("alt-right", HistoryForward, window),
        KeyBinding::new("up", RowUp, rows),
        KeyBinding::new("down", RowDown, rows),
        KeyBinding::new("enter", EditRow, rows),
        KeyBinding::new("up", KeyRowUp, keys),
        KeyBinding::new("down", KeyRowDown, keys),
        KeyBinding::new("enter", CaptureKey, keys),
        KeyBinding::new("delete", RemoveKey, keys),
        KeyBinding::new("r", ResetKeys, keys),
    ]
}

/// Named keys GPUI knows on Windows, besides single characters and F keys.
const NAMED: &[&str] = &[
    "space",
    "tab",
    "enter",
    "escape",
    "backspace",
    "delete",
    "insert",
    "home",
    "end",
    "pageup",
    "pagedown",
    "up",
    "down",
    "left",
    "right",
];

fn is_f_key(key: &str) -> bool {
    key.strip_prefix('f')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number))
}

/// One key in the form GPUI writes it, or `None` for text that is not one key.
/// GPUI takes any word as a key name, and a key named `blah` never fires, so
/// the name is checked here.
pub(super) fn normal(text: &str) -> Option<String> {
    if text.trim().contains(char::is_whitespace) {
        // A sequence of keys.
        return None;
    }
    let keystroke = Keystroke::parse(text.trim()).ok()?;
    let key = keystroke.key.as_str();
    (key.chars().count() == 1 || NAMED.contains(&key) || is_f_key(key)).then(|| keystroke.unparse())
}

/// Whether a key works everywhere, text inputs included. `shift` alone does
/// not count, because `shift-a` types `A`.
fn works_in_inputs(key: &str) -> bool {
    Keystroke::parse(key).is_ok_and(|keystroke| {
        keystroke.modifiers.control || keystroke.modifiers.alt || is_f_key(&keystroke.key)
    })
}

fn context(scope: Scope, key: &str) -> String {
    let base = match scope {
        Scope::Window => "FileBuddy",
        Scope::Rows => "Rows",
    };
    if works_in_inputs(key) {
        base.to_string()
    } else {
        format!("{base} && !Input")
    }
}

/// The defaults joined with the changes.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Keymap {
    /// The keys of each action, in the order of `ACTIONS`.
    keys: Vec<Vec<String>>,
    /// Entries that could not apply.
    pub(super) skipped: Vec<String>,
    /// Keys that two actions asked for. The key stays with the first.
    pub(super) conflicts: Vec<String>,
}

impl Keymap {
    /// What the header and the Keys section say.
    pub(super) fn problems(&self) -> Vec<String> {
        self.skipped
            .iter()
            .chain(&self.conflicts)
            .cloned()
            .collect()
    }
}

/// The keys of an action as the user set them, or its defaults.
pub(super) fn keys_of(action: &Action, changes: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let texts: Vec<&str> = match changes.get(action.name) {
        Some(keys) => keys.iter().map(String::as_str).collect(),
        None => action.defaults.to_vec(),
    };
    let mut keys = Vec::new();
    for key in texts.into_iter().filter_map(normal) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

pub(super) fn resolve(changes: &BTreeMap<String, Vec<String>>) -> Keymap {
    let mut skipped = Vec::new();
    for (name, keys) in changes {
        if !ACTIONS.iter().any(|action| action.name == name) {
            skipped.push(if FIXED.iter().any(|(_, fixed)| fixed == name) {
                format!("{name} cannot change")
            } else {
                format!("unknown action {name}")
            });
            continue;
        }
        for key in keys.iter().filter(|key| normal(key).is_none()) {
            skipped.push(format!("{key:?} for {name} is not a key"));
        }
    }

    let mut keys: Vec<Vec<String>> = ACTIONS
        .iter()
        .map(|action| keys_of(action, changes))
        .collect();

    // The defaults hold their keys first, so a change that takes a key from
    // another action is the one that loses it. The rows are a context inside
    // the window, and their keys win there, so a key can have one action in
    // each.
    let mut order: Vec<usize> = (0..ACTIONS.len()).collect();
    order.sort_by_key(|at| changes.contains_key(ACTIONS[*at].name));
    let mut holder: HashMap<(Scope, String), usize> = HashMap::new();
    let mut conflicts = Vec::new();
    for at in order {
        let name = ACTIONS[at].name;
        keys[at].retain(|key| {
            if let Some((_, fixed)) = FIXED
                .iter()
                .find(|(fixed, _)| normal(fixed).as_ref() == Some(key))
            {
                conflicts.push(format!(
                    "{key} is fixed to {fixed}, so {name} cannot have it"
                ));
                return false;
            }
            let scope = ACTIONS[at].scope;
            if let Some(other) = holder.get(&(scope, key.clone())) {
                conflicts.push(format!(
                    "{key} is on both {} and {name}",
                    ACTIONS[*other].name
                ));
                return false;
            }
            holder.insert((scope, key.clone()), at);
            true
        });
    }

    Keymap {
        keys,
        skipped,
        conflicts,
    }
}

/// The keys of gpui-component, as its `init` bound them. Clearing the keymap
/// removes them too, so they are bound again from this.
struct ComponentKeys(Vec<KeyBinding>);

impl Global for ComponentKeys {}

/// Bind every key again: gpui-component's, the fixed ones, the defaults and the
/// changes. Returns what could not apply.
pub fn apply(changes: &BTreeMap<String, Vec<String>>, cx: &mut App) -> Vec<String> {
    if !cx.has_global::<ComponentKeys>() {
        let component = cx.key_bindings().borrow().bindings().cloned().collect();
        cx.set_global(ComponentKeys(component));
    }
    let component = cx.global::<ComponentKeys>().0.clone();
    let keymap = resolve(changes);

    cx.clear_key_bindings();
    cx.bind_keys(component);
    cx.bind_keys(fixed_bindings());
    cx.bind_keys(ACTIONS.iter().zip(&keymap.keys).flat_map(|(action, keys)| {
        keys.iter()
            .map(|key| (action.bind)(key, Some(&context(action.scope, key))))
    }));
    keymap.problems()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(entries: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        entries
            .iter()
            .map(|(name, keys)| {
                (
                    name.to_string(),
                    keys.iter().map(|k| k.to_string()).collect(),
                )
            })
            .collect()
    }

    fn keys(keymap: &Keymap, name: &str) -> Vec<String> {
        let at = ACTIONS
            .iter()
            .position(|action| action.name == name)
            .unwrap();
        keymap.keys[at].clone()
    }

    #[test]
    fn the_defaults_are_keys_and_do_not_conflict() {
        for action in ACTIONS {
            for key in action.defaults {
                assert!(normal(key).is_some(), "{key} of {}", action.name);
            }
        }
        assert_eq!(resolve(&BTreeMap::new()).problems(), Vec::<String>::new());
    }

    #[test]
    fn the_context_comes_from_the_key() {
        assert_eq!(context(Scope::Window, "ctrl-e"), "FileBuddy");
        assert_eq!(context(Scope::Window, "alt-1"), "FileBuddy");
        assert_eq!(context(Scope::Window, "f2"), "FileBuddy");
        assert_eq!(context(Scope::Window, "r"), "FileBuddy && !Input");
        assert_eq!(context(Scope::Window, "shift-left"), "FileBuddy && !Input");
        assert_eq!(context(Scope::Window, "space"), "FileBuddy && !Input");
        assert_eq!(context(Scope::Rows, "l"), "Rows && !Input");
        assert_eq!(context(Scope::Rows, "ctrl-n"), "Rows");
    }

    #[test]
    fn a_change_replaces_the_defaults_and_an_empty_list_unbinds() {
        let keymap = resolve(&changes(&[("rename_file", &["F2"]), ("toggle_table", &[])]));
        assert_eq!(keys(&keymap, "rename_file"), ["f2"]);
        assert!(keys(&keymap, "toggle_table").is_empty());
        assert_eq!(keys(&keymap, "toggle_mark"), ["x"]);
        assert!(keymap.problems().is_empty());
    }

    #[test]
    fn a_key_taken_from_another_action_is_a_conflict_that_names_both() {
        let keymap = resolve(&changes(&[("rename_file", &["x", "f2"])]));
        assert_eq!(
            keymap.conflicts,
            ["x is on both toggle_mark and rename_file"]
        );
        assert_eq!(keys(&keymap, "toggle_mark"), ["x"]);
        assert_eq!(keys(&keymap, "rename_file"), ["f2"]);
    }

    #[test]
    fn the_rows_and_the_window_can_share_a_key() {
        let keymap = resolve(&BTreeMap::new());
        assert_eq!(keys(&keymap, "trash_files"), ["delete"]);
        assert_eq!(keys(&keymap, "delete_row"), ["delete"]);
        let keymap = resolve(&changes(&[("rescan", &["delete"])]));
        assert_eq!(
            keymap.conflicts,
            ["delete is on both trash_files and rescan"]
        );
    }

    #[test]
    fn two_changes_on_one_key_conflict() {
        let keymap = resolve(&changes(&[("tag_files", &["q"]), ("toggle_mark", &["q"])]));
        assert_eq!(keymap.conflicts.len(), 1);
        assert!(keymap.conflicts[0].starts_with("q is on both"));
    }

    #[test]
    fn a_fixed_key_cannot_be_taken() {
        let keymap = resolve(&changes(&[("rescan", &["ctrl-c", "alt-left"])]));
        assert_eq!(
            keymap.conflicts,
            [
                "ctrl-c is fixed to copy_file, so rescan cannot have it",
                "alt-left is fixed to history_back, so rescan cannot have it",
            ]
        );
        assert!(keys(&keymap, "rescan").is_empty());
    }

    #[test]
    fn a_bad_entry_is_skipped_and_the_rest_apply() {
        let keymap = resolve(&changes(&[
            ("copy_file", &["ctrl-k"]),
            ("fly_away", &["k"]),
            ("rescan", &["ctrl-x-y", "blah", "ctrl-k ctrl-d", "f6"]),
            ("tag_files", &["g"]),
        ]));
        assert_eq!(
            keymap.skipped,
            [
                "copy_file cannot change",
                "unknown action fly_away",
                "\"ctrl-x-y\" for rescan is not a key",
                "\"blah\" for rescan is not a key",
                "\"ctrl-k ctrl-d\" for rescan is not a key",
            ]
        );
        assert_eq!(keys(&keymap, "rescan"), ["f6"]);
        assert_eq!(keys(&keymap, "tag_files"), ["g"]);
    }
}
