//! Add captions: the speech segments inside the range become texts at the
//! bottom center, one segment at a time.

use super::{BOX_PADDING, Edit, LINE_HEIGHT, Text};
use crate::speech::Segment;

/// The Small size of the Text tab.
const SIZE: f64 = 0.05;
/// The bottom of a caption's box, from the top of the frame.
const BOTTOM: f64 = 0.92;
/// A line fits this share of the frame width.
const LINE_SHARE: f64 = 0.9;
const MAX_LINES: usize = 2;

impl Edit {
    /// One text per segment inside the range, cut to the range ends.
    /// `em_width` measures a line in ems of the export font. A segment too
    /// long for two lines becomes consecutive texts, its time shared by the
    /// length of their words. Returns how many texts it added.
    pub fn add_captions(&mut self, segments: &[Segment], em_width: impl Fn(&str) -> f64) -> usize {
        let (width, height) = self.output_size();
        let limit = LINE_SHARE * f64::from(width) / (SIZE * f64::from(height));
        let before = self.texts.len();
        for segment in segments {
            let start_ms = (segment.start_ms.max(0) as u64).max(self.range.start_ms);
            let end_ms = (segment.end_ms.max(0) as u64).min(self.range.end_ms);
            if end_ms <= start_ms {
                continue;
            }
            let lines = wrap(segment.text.trim(), limit, &em_width);
            let chunks: Vec<String> = lines.chunks(MAX_LINES).map(|c| c.join("\n")).collect();
            let total: usize = chunks.iter().map(|c| c.len()).sum();
            let mut done = 0;
            for chunk in chunks {
                let from = start_ms + (end_ms - start_ms) * done as u64 / total as u64;
                done += chunk.len();
                let to = start_ms + (end_ms - start_ms) * done as u64 / total as u64;
                let count = chunk.split('\n').count() as f64;
                let box_height = SIZE * (count * LINE_HEIGHT + 2. * BOX_PADDING);
                self.texts.push(Text {
                    words: chunk,
                    x: 0.5,
                    y: BOTTOM - box_height / 2.,
                    size: SIZE,
                    boxed: true,
                    start_ms: from,
                    end_ms: to,
                });
            }
        }
        self.texts.len() - before
    }
}

/// Words into lines no wider than `limit` ems. A word wider than that has a
/// line of its own.
fn wrap(text: &str, limit: f64, em_width: impl Fn(&str) -> f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if em_width(&format!("{line} {word}")) <= limit => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(start_ms: i64, end_ms: i64, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms,
            text: text.into(),
            confidence: None,
            no_speech_prob: None,
        }
    }

    /// Half an em for each character.
    fn half_em(line: &str) -> f64 {
        line.chars().count() as f64 * 0.5
    }

    /// 1000 x 500 at 5%: a line fits 0.9 * 1000 / 25 = 36 ems, 72 characters.
    fn edit() -> Edit {
        let mut edit = Edit::new("a.mp4".into(), "a.mp4".into(), 10_000, (1000, 500));
        edit.range.start_ms = 2_000;
        edit.range.end_ms = 8_000;
        edit
    }

    #[test]
    fn segments_are_cut_to_the_range_at_any_speed() {
        let mut edit = edit();
        edit.speed = 2.;
        let added = edit.add_captions(
            &[
                segment(0, 1_500, "before"),
                segment(1_000, 3_000, "across the in point"),
                segment(4_000, 5_000, "inside"),
                segment(7_000, 9_000, "across the out point"),
                segment(8_000, 9_000, "after"),
            ],
            half_em,
        );
        assert_eq!(added, 3);
        let times: Vec<_> = edit.texts.iter().map(|t| (t.start_ms, t.end_ms)).collect();
        assert_eq!(times, [(2_000, 3_000), (4_000, 5_000), (7_000, 8_000)]);
        // Texts hold source time. The export puts them at half of it from the
        // in point.
        let windows: Vec<_> = edit
            .texts
            .iter()
            .map(|t| edit.text_window(t).unwrap())
            .map(|(start, end)| (start / edit.speed, end / edit.speed))
            .collect();
        assert_eq!(windows, [(0., 0.5), (1., 1.5), (2.5, 3.)]);
        assert!(edit.texts.iter().all(|t| t.boxed && t.x == 0.5));
    }

    #[test]
    fn a_long_segment_wraps_to_two_lines() {
        let words = ["word"; 20].join(" ");
        assert_eq!(wrap(&words, 36., half_em), [
            ["word"; 14].join(" "),
            ["word"; 6].join(" "),
        ]);
        let mut edit = edit();
        edit.add_captions(&[segment(2_000, 3_000, &words)], half_em);
        assert_eq!(edit.texts.len(), 1);
        assert_eq!(edit.texts[0].words.split('\n').count(), 2);
    }

    #[test]
    fn a_segment_past_two_lines_shares_its_time() {
        let words = ["word"; 42].join(" ");
        let mut edit = edit();
        edit.add_captions(&[segment(2_000, 4_000, &words)], half_em);
        let texts: Vec<_> = edit
            .texts
            .iter()
            .map(|t| (t.start_ms, t.end_ms, t.words.split('\n').count()))
            .collect();
        // 28 words in two lines, 14 in one: two thirds of the time and a third.
        assert_eq!(texts, [(2_000, 3_336, 2), (3_336, 4_000, 1)]);
    }

    #[test]
    fn a_word_wider_than_the_line_has_its_own() {
        let long = "x".repeat(80);
        assert_eq!(wrap(&format!("a {long} b"), 36., half_em), ["a", &long, "b"]);
    }
}
