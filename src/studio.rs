//! The edit the Studio holds: a source and the range cut from it. Nothing of it
//! goes to the database. `ffmpeg::export` makes a file from it.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// The file as the index knows it. The export takes its name from it.
    pub source: PathBuf,
    /// What FFmpeg reads: the file, or its proxy.
    pub input: PathBuf,
    pub duration_ms: u64,
    pub range: Range,
    pub quality: Quality,
}

impl Edit {
    /// The whole source, at the default quality.
    pub fn new(source: PathBuf, input: PathBuf, duration_ms: u64) -> Self {
        Self {
            source,
            input,
            duration_ms,
            range: Range {
                start_ms: 0,
                end_ms: duration_ms,
            },
            quality: Quality::High,
        }
    }

    /// `[`. An in point at or past the out point moves the out point to the end.
    pub fn set_start(&mut self, at_ms: u64) {
        self.range.start_ms = at_ms.min(self.duration_ms);
        if self.range.end_ms <= self.range.start_ms {
            self.range.end_ms = self.duration_ms;
        }
    }

    /// `]`. An out point at or before the in point moves the in point to the
    /// start.
    pub fn set_end(&mut self, at_ms: u64) {
        self.range.end_ms = at_ms.min(self.duration_ms);
        if self.range.end_ms <= self.range.start_ms {
            self.range.start_ms = 0;
        }
    }

    /// A drag on a handle. It stops at the other handle.
    pub fn drag(&mut self, edge: Edge, at_ms: u64) {
        match edge {
            Edge::Start => self.range.start_ms = at_ms.min(self.range.end_ms),
            Edge::End => {
                self.range.end_ms = at_ms.clamp(self.range.start_ms, self.duration_ms);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Range {
    pub fn len_ms(self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    pub fn at(self, edge: Edge) -> u64 {
        match edge {
            Edge::Start => self.start_ms,
            Edge::End => self.end_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Best,
    High,
    Medium,
    Low,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Best, Quality::High, Quality::Medium, Quality::Low];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Best => "Best",
            Quality::High => "High",
            Quality::Medium => "Medium",
            Quality::Low => "Low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Mp4,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Mp4 => "mp4",
        }
    }
}

/// `<source stem> <in>-<out>.<ext>`, for example `cat jump 0m03-0m07.mp4`.
pub fn export_name(source: &Path, range: Range, format: Format) -> String {
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!(
        "{stem} {}-{}.{}",
        stamp(range.start_ms),
        stamp(range.end_ms),
        format.extension()
    )
}

/// `0m03`, or `1h02m03` from an hour.
fn stamp(ms: u64) -> String {
    let total = ms / 1000;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}")
    } else {
        format!("{minutes}m{seconds:02}")
    }
}

/// `name` in `folder`, or with ` (2)`, ` (3)` and so on if that name exists.
pub fn unique_path(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (name, String::new()),
    };
    (2..)
        .map(|n| folder.join(format!("{stem} ({n}){extension}")))
        .find(|path| !path.exists())
        .expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit() -> Edit {
        Edit::new("a.mp4".into(), "a.mp4".into(), 10_000)
    }

    #[test]
    fn the_name_holds_the_stem_and_the_range() {
        let range = Range {
            start_ms: 3_400,
            end_ms: 7_900,
        };
        assert_eq!(
            export_name(Path::new("D:/clips/cat jump.mp4"), range, Format::Mp4),
            "cat jump 0m03-0m07.mp4"
        );
        let range = Range {
            start_ms: 62_000,
            end_ms: 3_723_000,
        };
        assert_eq!(
            export_name(Path::new("a.b.webm"), range, Format::Mp4),
            "a.b 1m02-1h02m03.mp4"
        );
    }

    #[test]
    fn a_name_that_exists_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let name = "cat jump 0m03-0m07.mp4";
        assert_eq!(unique_path(dir.path(), name), dir.path().join(name));
        std::fs::write(dir.path().join(name), b"").unwrap();
        let second = dir.path().join("cat jump 0m03-0m07 (2).mp4");
        assert_eq!(unique_path(dir.path(), name), second);
        std::fs::write(&second, b"").unwrap();
        assert_eq!(
            unique_path(dir.path(), name),
            dir.path().join("cat jump 0m03-0m07 (3).mp4")
        );
    }

    #[test]
    fn the_keys_push_the_other_point_out_of_the_way() {
        let mut edit = edit();
        edit.set_end(4_000);
        edit.set_start(6_000);
        assert_eq!((edit.range.start_ms, edit.range.end_ms), (6_000, 10_000));
        edit.set_end(2_000);
        assert_eq!((edit.range.start_ms, edit.range.end_ms), (0, 2_000));
        edit.set_end(20_000);
        assert_eq!(edit.range.end_ms, 10_000);
    }

    #[test]
    fn a_drag_stops_at_the_other_handle() {
        let mut edit = edit();
        edit.set_start(3_000);
        edit.set_end(5_000);
        edit.drag(Edge::Start, 8_000);
        assert_eq!(edit.range.start_ms, 5_000);
        edit.drag(Edge::End, 1_000);
        assert_eq!(edit.range.end_ms, 5_000);
        edit.drag(Edge::End, 12_000);
        assert_eq!(edit.range.end_ms, 10_000);
    }
}
