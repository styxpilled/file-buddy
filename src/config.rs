//! The TOML configuration file, written next to the executable on first run.

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
}

/// The bm25 column weights. The filename outranks machine text because the user
/// controls it. Tags outrank the filename because the user applies them on
/// purpose.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Weights {
    pub tags: f64,
    pub filename: f64,
    pub speech: f64,
    pub ocr: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            tags: 20.0,
            filename: 10.0,
            speech: 3.0,
            ocr: 1.0,
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
    }

    #[test]
    fn weights_and_ignore_are_optional() {
        let config = Config::parse(r#"roots = ["D:/clips"]"#).unwrap();
        assert_eq!(config.weights.ocr, 1.0);
        assert_eq!(config.ignore.len(), 2);
    }

    #[test]
    fn a_written_file_reads_back_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file-buddy.toml");
        let written = Config::load_or_create(&path).unwrap();
        let read = Config::load_or_create(&path).unwrap();
        assert_eq!(written.roots, read.roots);
        assert_eq!(written.weights.speech, read.weights.speech);
    }

    #[test]
    fn ignore_globs_match_a_nested_path() {
        let config = Config::default();
        let set = config.ignore_set().unwrap();
        assert!(set.is_match("D:/clips/thumbs/a.png"));
        assert!(!set.is_match("D:/clips/a.png"));
    }
}
