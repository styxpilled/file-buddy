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

    /// What the window calls the kind. `as_str` is what the database holds.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Video => "video",
            Kind::Animated => "gif",
            Kind::Still => "image",
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

/// A length as the window shows it: `9.4s` under a minute, `5:01` from there,
/// and `1:02:30` from an hour.
pub fn duration(seconds: f64) -> String {
    let tenths = (seconds * 10.).round();
    if tenths < 600. {
        return format!("{:.1}s", tenths / 10.);
    }
    let total = seconds.round() as u64;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
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
    fn a_duration_keeps_its_tenths_under_a_minute() {
        assert_eq!(duration(9.44), "9.4s");
        assert_eq!(duration(59.94), "59.9s");
        // Rounds up to a whole minute, so it reads as one.
        assert_eq!(duration(59.96), "1:00");
        assert_eq!(duration(301.0), "5:01");
        assert_eq!(duration(3750.0), "1:02:30");
    }

    #[test]
    fn extension_decides_the_first_guess() {
        assert_eq!(Kind::from_path(Path::new("a/b.MP4")), Some(Kind::Video));
        assert_eq!(Kind::from_path(Path::new("a/b.gif")), Some(Kind::Animated));
        assert_eq!(Kind::from_path(Path::new("a/b.webp")), Some(Kind::Still));
        assert_eq!(Kind::from_path(Path::new("a/b.txt")), None);
        assert_eq!(Kind::from_path(Path::new("a/b")), None);
    }
}
