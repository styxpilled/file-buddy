pub mod filename;
pub mod hash;

use std::path::Path;
use std::str::FromStr;

/// What the file is, which decides the stages it runs and how the grid shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Video,
    Animated,
    Still,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Video => "video",
            Kind::Animated => "animated",
            Kind::Still => "still",
        }
    }

    /// The first guess, from the extension. A probe can move a file from
    /// `Still` to `Animated`, because an animated WebP has a still extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "wmv" => Kind::Video,
            "gif" | "apng" => Kind::Animated,
            "png" | "jpg" | "jpeg" | "webp" | "bmp" | "avif" | "jfif" => Kind::Still,
            _ => return None,
        })
    }
}

impl FromStr for Kind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "video" => Kind::Video,
            "animated" => Kind::Animated,
            "still" => Kind::Still,
            other => anyhow::bail!("unknown media kind {other:?}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_decides_the_first_guess() {
        assert_eq!(Kind::from_path(Path::new("a/b.MP4")), Some(Kind::Video));
        assert_eq!(Kind::from_path(Path::new("a/b.gif")), Some(Kind::Animated));
        assert_eq!(Kind::from_path(Path::new("a/b.webp")), Some(Kind::Still));
        assert_eq!(Kind::from_path(Path::new("a/b.txt")), None);
        assert_eq!(Kind::from_path(Path::new("a/b")), None);
    }
}
