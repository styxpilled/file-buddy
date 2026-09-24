//! The TOML configuration file, written next to the executable on first run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub roots: Vec<PathBuf>,
    #[serde(default = "default_ignore")]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub weights: Weights,
    #[serde(default)]
    pub paused: Paused,
    /// The window keys the user changed, by action. An action that is not here
    /// keeps its default keys, and an empty list unbinds it. The window checks
    /// the entries, since only it knows the actions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, Vec<String>>,
}

/// Stages that start no new job. A job that runs when one is paused finishes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Paused {
    pub ocr: bool,
    pub clip: bool,
    pub whisper: bool,
}

/// The bm25 column weights. The filename outranks machine text because the user
/// controls it. Tags outrank the filename because the user applies them on
/// purpose.
///
/// `visual` is of another kind: the weight of the visual list against the text
/// list, which counts 1, when the two merge.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Weights {
    pub tags: f64,
    pub filename: f64,
    pub speech: f64,
    pub ocr: f64,
    #[serde(default = "default_visual_weight")]
    pub visual: f64,
}

/// Below 1, so a word in a filename or a tag still puts that file first.
fn default_visual_weight() -> f64 {
    0.5
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            tags: 20.0,
            filename: 10.0,
            speech: 3.0,
            ocr: 1.0,
            visual: default_visual_weight(),
        }
    }
}

fn default_ignore() -> Vec<String> {
    vec!["**/thumbs/**".into(), "**/.git/**".into()]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: vec![PathBuf::from("D:/clips")],
            ignore: default_ignore(),
            weights: Weights::default(),
            paused: Paused::default(),
            keys: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Read the file, or write the default one and return it.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if !path.exists() {
            let config = Config::default();
            config.write(path)?;
            return Ok(config);
        }
        Self::parse(
            &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
        )
    }

    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("parse the configuration")
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let text = toml::to_string_pretty(self).context("serialize the configuration")?;
        std::fs::write(path, text).with_context(|| format!("write {}", path.display()))
    }

    /// A configuration from the Settings page, checked before it can be written.
    /// `ignore` holds one glob per line; `weights` are in column order. The
    /// switches that pause a stage save at once, so they are not part of it.
    pub fn edited(
        roots: Vec<PathBuf>,
        ignore: &str,
        weights: [&str; 5],
        paused: Paused,
        keys: BTreeMap<String, Vec<String>>,
    ) -> Result<Self> {
        if roots.is_empty() {
            anyhow::bail!("add at least one folder");
        }
        let mut parsed = [0.0; 5];
        for ((slot, text), name) in parsed.iter_mut().zip(weights).zip([
            "tags",
            "filename",
            "speech",
            "screen text",
            "visual",
        ]) {
            *slot = text
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .with_context(|| format!("the {name} weight must be a number of 0 or more"))?;
        }
        let config = Self {
            roots,
            ignore: ignore
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(String::from)
                .collect(),
            weights: Weights {
                tags: parsed[0],
                filename: parsed[1],
                speech: parsed[2],
                ocr: parsed[3],
                visual: parsed[4],
            },
            paused,
            keys,
        };
        config.ignore_set()?;
        Ok(config)
    }

    /// The compiled ignore globs. Matching is case insensitive, and a backslash
    /// in a Windows path is matched as a separator.
    pub fn ignore_set(&self) -> Result<GlobSet> {
        let mut builder = GlobSetBuilder::new();
        for pattern in &self.ignore {
            let glob =
                Glob::new(pattern).with_context(|| format!("bad ignore glob {pattern:?}"))?;
            builder.add(glob);
        }
        builder.build().context("build the ignore globs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_file() {
        let config = Config::parse(
            r#"
roots = ["D:/clips"]
ignore = ["**/thumbs/**", "**/.git/**"]
weights = { tags = 20.0, filename = 10.0, speech = 3.0, ocr = 1.0 }
"#,
        )
        .unwrap();
        assert_eq!(config.roots, vec![PathBuf::from("D:/clips")]);
        assert_eq!(config.weights.tags, 20.0);
        // A file from before the visual weight gets its default.
        assert_eq!(config.weights.visual, 0.5);
    }

    #[test]
    fn weights_and_ignore_are_optional() {
        let config = Config::parse(r#"roots = ["D:/clips"]"#).unwrap();
        assert_eq!(config.weights.ocr, 1.0);
        assert_eq!(config.ignore.len(), 2);
        assert_eq!(config.paused, Paused::default());
    }

    #[test]
    fn a_written_file_reads_back_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file-buddy.toml");
        let written = Config::load_or_create(&path).unwrap();
        let read = Config::load_or_create(&path).unwrap();
        assert_eq!(written.roots, read.roots);
        assert_eq!(written.weights.speech, read.weights.speech);

        let paused = Config {
            paused: Paused {
                ocr: false,
                clip: false,
                whisper: true,
            },
            ..written
        };
        paused.write(&path).unwrap();
        assert_eq!(Config::load_or_create(&path).unwrap().paused, paused.paused);
    }

    #[test]
    fn an_edited_configuration_is_checked() {
        let roots = vec![PathBuf::from("D:/clips")];
        let config = Config::edited(
            roots.clone(),
            "**/thumbs/**\n\n  **/.git/**  \n",
            ["20", "10.5", " 3 ", "0", "0.5"],
            Paused::default(),
            BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(config.ignore, ["**/thumbs/**", "**/.git/**"]);
        assert_eq!(config.weights.filename, 10.5);
        assert_eq!(config.weights.ocr, 0.0);
        assert_eq!(config.weights.visual, 0.5);

        let error = |roots, ignore, weights| {
            Config::edited(roots, ignore, weights, Paused::default(), BTreeMap::new())
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            error(vec![], "", ["1", "1", "1", "1", "1"]),
            "add at least one folder"
        );
        assert_eq!(
            error(roots.clone(), "", ["1", "-1", "1", "1", "1"]),
            "the filename weight must be a number of 0 or more"
        );
        assert_eq!(
            error(roots.clone(), "", ["1", "1", "1", "lots", "1"]),
            "the screen text weight must be a number of 0 or more"
        );
        assert!(error(roots, "a/{b", ["1", "1", "1", "1", "1"]).starts_with("bad ignore glob"));
    }

    #[test]
    fn changed_keys_read_back_and_no_change_writes_no_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file-buddy.toml");
        let config = Config::load_or_create(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("[keys]"));

        let keys = BTreeMap::from([
            (
                "rename_file".to_string(),
                vec!["r".to_string(), "f2".to_string()],
            ),
            ("toggle_table".to_string(), vec![]),
        ]);
        Config {
            keys: keys.clone(),
            ..config
        }
        .write(&path)
        .unwrap();
        assert_eq!(Config::load_or_create(&path).unwrap().keys, keys);
    }

    #[test]
    fn ignore_globs_match_a_nested_path() {
        let config = Config::default();
        let set = config.ignore_set().unwrap();
        assert!(set.is_match("D:/clips/thumbs/a.png"));
        assert!(!set.is_match("D:/clips/a.png"));
    }
}
