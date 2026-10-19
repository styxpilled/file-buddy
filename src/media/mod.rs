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

/// A file size: `342 KB`, `5.0 MB`, `1.2 GB`. A file under 1 KB shows as 1 KB,
/// not 0.
pub fn bytes(size: u64) -> String {
    const KB: f64 = 1024.;
    const MB: f64 = KB * 1024.;
    const GB: f64 = MB * 1024.;
    let size = size as f64;
    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else {
        format!("{:.0} KB", (size / KB).ceil())
    }
}

/// A point in a clip: `0:07`, `1:42`, or `1:02:30` from an hour.
pub fn clock(ms: i64) -> String {
    let total = ms.max(0) / 1000;
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
    fn the_numbers_are_short() {
        assert_eq!(bytes(1), "1 KB");
        assert_eq!(bytes(350_000), "342 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
    }

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
    fn a_point_in_a_clip_counts_whole_seconds() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(7_999), "0:07");
        assert_eq!(clock(102_000), "1:42");
        assert_eq!(clock(3_750_000), "1:02:30");
        assert_eq!(clock(-5), "0:00");
    }

    #[test]
    fn extension_decides_the_first_guess() {
        assert_eq!(Kind::from_path(Path::new(r"a\b.MP4")), Some(Kind::Video));
        assert_eq!(Kind::from_path(Path::new(r"a\b.gif")), Some(Kind::Animated));
        assert_eq!(Kind::from_path(Path::new(r"a\b.webp")), Some(Kind::Still));
        assert_eq!(Kind::from_path(Path::new(r"a\b.txt")), None);
        assert_eq!(Kind::from_path(Path::new("a/b")), None);
    }
}
