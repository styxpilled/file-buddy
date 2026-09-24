//! On-screen text. The recognizer almost never invents text, so the cleaning
//! here is only about volume: a chart of numbers gave 311 lines in the spike,
//! and the same caption comes back in every frame that shows it.

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::Connection;

use crate::db::files::FileRow;
use crate::db::ocr::Line;
use crate::db::{fts, ocr as ocr_rows};
use crate::ffmpeg::frames;
use crate::ocr;

pub fn run(conn: &Connection, source: &FileRow) -> Result<()> {
    let frames = frames::sample(&source.path, &source.hash, source.duration)?;
    let engine = ocr::engine()?;

    let mut lines = Vec::new();
    let mut read = 0;
    let mut failure = None;
    for (time_ms, frame) in &frames.paths {
        match engine.read(frame) {
            Ok(found) => {
                read += 1;
                lines.extend(found.into_iter().map(|text| Line::new(*time_ms, text)));
            }
            // One unreadable frame is not a failed file. Every frame is.
            Err(e) => {
                log::warn!("ocr failed on {}: {e:#}", frame.display());
                failure = Some(e);
            }
        }
    }
    if read == 0
        && let Some(e) = failure
    {
        return Err(e);
    }

    let kept = clean(lines);
    if kept.is_empty() {
        log::info!("no text found in {}", source.path.display());
    }
    ocr_rows::replace(conn, &source.hash, &kept)?;
    fts::refresh_hash(conn, &source.hash)
}

/// A line with fewer letters than this says nothing worth indexing.
const MIN_LETTERS: usize = 3;

/// Every line the recognizer read, each one once, at the first frame that
/// shows it. What is worth searching is decided where the index is built, in
/// `ocr::noise`, so a change to that rule does not send every file through the
/// recognizer again.
fn clean(lines: Vec<Line>) -> Vec<Line> {
    let mut seen = HashSet::new();
    lines
        .into_iter()
        .map(|line| Line {
            text: line.text.trim().to_string(),
            ..line
        })
        .filter(|line| is_text(&line.text) && seen.insert(line.text.to_lowercase()))
        .collect()
}

/// Whether a line is language and not chart noise. Numbers, axis labels and
/// stray marks nearly all fail one of these two tests.
fn is_text(line: &str) -> bool {
    let letters = line.chars().filter(|c| c.is_alphabetic()).count();
    let solid = line.chars().filter(|c| !c.is_whitespace()).count();
    letters >= MIN_LETTERS && letters * 2 >= solid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean_all(lines: &[&str]) -> Vec<String> {
        clean(lines.iter().map(|l| Line::new(0, *l)).collect())
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    #[test]
    fn keeps_a_caption() {
        assert_eq!(
            clean_all(&["Meet the silky anteater"]),
            vec!["Meet the silky anteater"]
        );
    }

    #[test]
    fn drops_numbers_and_short_fragments() {
        assert!(clean_all(&["1.4372", "0.05 0.10 0.15", "IT", "|", "12 x 4"]).is_empty());
    }

    #[test]
    fn keeps_a_number_that_sits_in_a_sentence() {
        assert_eq!(
            clean_all(&["we lost 40 percent"]),
            vec!["we lost 40 percent"]
        );
    }

    #[test]
    fn a_line_keeps_the_time_of_the_first_frame_that_shows_it() {
        let lines = clean(vec![
            Line::new(0, "the intro"),
            Line::new(2000, "Hold on there"),
            Line::new(4000, "hold on there"),
        ]);
        assert_eq!(
            lines,
            vec![Line::new(0, "the intro"), Line::new(2000, "Hold on there")]
        );
    }

    #[test]
    fn the_same_caption_in_every_frame_is_indexed_once() {
        assert_eq!(
            clean_all(&["Hold on there", " hold on there ", "HOLD ON THERE"]),
            vec!["Hold on there"]
        );
    }
}
