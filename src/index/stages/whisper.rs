//! Speech. The filter gives no probabilities, so the text is cleaned by rules:
//! known hallucination phrases, sound tags, and the short last window that the
//! model fills with words that were never said.

use anyhow::Result;
use rusqlite::Connection;

use crate::db::files::FileRow;
use crate::db::{fts, segments};
use crate::ffmpeg::whisper::{self, Segment, WINDOW_SECONDS};
use crate::paths;

/// A last window shorter than this holds no speech worth keeping, and the model
/// hallucinates in it ("Thank you.", "Bye.").
const SHORT_TAIL_MS: i64 = 2000;
/// The filter's window edges move earlier by about 23 ms per window. This
/// covers ten windows, which is the longest file that gets a transcript.
const WINDOW_DRIFT_MS: i64 = 500;

/// Phrases the model produces from silence and music, in the form `normalized`
/// gives. A segment is dropped only when this is all it says.
const HALLUCINATIONS: &[&str] = &[
    "thank you",
    "thank you very much",
    "thanks for watching",
    "thank you for watching",
    "thank you so much for watching",
    "please subscribe",
    "like and subscribe",
    "please like and subscribe",
    "subscribe to my channel",
    "bye",
    "you",
    "subtitles by the amara org community",
    "music",
    "applause",
    "laughter",
];

pub fn run(conn: &Connection, source: &FileRow) -> Result<()> {
    let model = paths::whisper_model()?;
    let kept = clean(whisper::transcribe(&source.path, &model)?, source.duration);
    if kept.is_empty() {
        log::info!("no speech found in {}", source.path.display());
    }
    // No segments is the record of "no speech found". The finished job keeps
    // the file from being transcribed again.
    segments::replace(conn, &source.hash, &kept)?;
    fts::refresh_hash(conn, &source.hash)
}

fn clean(segments: Vec<Segment>, duration: Option<f64>) -> Vec<Segment> {
    let duration_ms = duration.map(|d| (d * 1000.0) as i64);
    let tail_start_ms = duration_ms.and_then(short_tail_start);
    segments
        .into_iter()
        .filter_map(|mut segment| {
            if tail_start_ms.is_some_and(|tail| segment.start_ms >= tail - WINDOW_DRIFT_MS) {
                return None;
            }
            segment.text = segment
                .text
                .trim()
                .trim_start_matches('-')
                .trim()
                .to_string();
            let words = normalized(&without_sound_tags(&segment.text));
            if words.is_empty() || HALLUCINATIONS.contains(&words.as_str()) {
                return None;
            }
            if let Some(duration_ms) = duration_ms {
                segment.end_ms = segment.end_ms.min(duration_ms);
            }
            Some(segment)
        })
        .collect()
}

/// Where the last window starts, when it is too short to trust.
fn short_tail_start(duration_ms: i64) -> Option<i64> {
    let window_ms = i64::from(WINDOW_SECONDS) * 1000;
    let tail_ms = duration_ms % window_ms;
    (duration_ms > window_ms && tail_ms > 0 && tail_ms < SHORT_TAIL_MS)
        .then_some(duration_ms - tail_ms)
}

/// The text without `[Music]`, `(laughs)`, `*sighs*` and music notes.
fn without_sound_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut closing = None;
    for c in text.chars() {
        match closing {
            Some(end) if c == end => closing = None,
            Some(_) => {}
            None => match c {
                '[' => closing = Some(']'),
                '(' => closing = Some(')'),
                '*' => closing = Some('*'),
                '♪' | '♫' => {}
                _ => out.push(c),
            },
        }
    }
    out
}

/// Lowercase words, with punctuation removed and single spaces between them.
fn normalized(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(start_ms: i64, end_ms: i64, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms,
            text: text.into(),
        }
    }

    fn texts(segments: &[Segment]) -> Vec<&str> {
        segments.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn drops_a_known_hallucination_but_not_a_sentence_that_holds_it() {
        let kept = clean(
            vec![
                segment(0, 1000, " Thanks for watching!"),
                segment(1000, 2000, "Thank you for the beers."),
            ],
            Some(10.0),
        );
        assert_eq!(texts(&kept), vec!["Thank you for the beers."]);
    }

    #[test]
    fn drops_a_sound_tag_and_keeps_the_words_beside_one() {
        let kept = clean(
            vec![
                segment(0, 1000, "[Music]"),
                segment(1000, 2000, "(upbeat music) ♪"),
                segment(1500, 2000, "Music."),
                segment(2000, 3000, "[BLANK_AUDIO]"),
                segment(3000, 4000, "♪ hit the road jack ♪"),
            ],
            Some(10.0),
        );
        assert_eq!(texts(&kept), vec!["♪ hit the road jack ♪"]);
    }

    #[test]
    fn drops_the_segments_of_a_short_last_window() {
        // 31.42 seconds: the last window holds 1.42 seconds.
        let kept = clean(
            vec![
                segment(21460, 26540, "Will turn to whisper in your ear"),
                segment(29999, 59979, "Thank you for everything."),
            ],
            Some(31.42),
        );
        assert_eq!(texts(&kept), vec!["Will turn to whisper in your ear"]);
    }

    #[test]
    fn the_short_last_window_starts_where_the_filter_put_it() {
        // 60.12 seconds: the filter started the last window at 59976 ms.
        let kept = clean(
            vec![
                segment(51239, 59979, "Finally number 1."),
                segment(59976, 60476, "Okay."),
            ],
            Some(60.116),
        );
        assert_eq!(texts(&kept), vec!["Finally number 1."]);
    }

    #[test]
    fn keeps_a_long_last_window_and_ends_a_segment_at_the_end_of_the_file() {
        let kept = clean(vec![segment(30000, 40000, "Number two.")], Some(35.0));
        assert_eq!(kept, vec![segment(30000, 35000, "Number two.")]);
    }

    #[test]
    fn a_file_shorter_than_one_window_has_no_tail_to_drop() {
        let kept = clean(vec![segment(0, 1000, "Hello there.")], Some(1.5));
        assert_eq!(texts(&kept), vec!["Hello there."]);
    }

    #[test]
    fn strips_the_dash_of_a_speaker_change() {
        let kept = clean(vec![segment(0, 1000, "- Fuck you!")], None);
        assert_eq!(texts(&kept), vec!["Fuck you!"]);
    }
}
