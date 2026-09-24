//! Where in a clip a speech match is. FTS5 finds the file by the joined text of
//! its segments, so the segment is found again here, by the same words.

use crate::speech::Segment;

/// The segment that holds the most words of the query, the first of them on a
/// tie. `None` when no segment holds any. The words match as prefixes, as they
/// do in the index.
pub fn segment<'a>(segments: &'a [Segment], query: &str) -> Option<&'a Segment> {
    let terms: Vec<Vec<String>> = query
        .split_whitespace()
        .filter(|word| !matches!(word.split_once(':'), Some(("tag" | "tags", _))))
        .map(tokens)
        .filter(|term| !term.is_empty())
        .collect();
    let mut best = None;
    let mut most = 0;
    for segment in segments {
        let words = tokens(&segment.text);
        let held = terms
            .iter()
            .filter(|term| {
                term.iter()
                    .all(|part| words.iter().any(|word| word.starts_with(part.as_str())))
            })
            .count();
        if held > most {
            most = held;
            best = Some(segment);
        }
    }
    best
}

/// Lowercase runs of letters and digits, as `unicode61` splits text.
fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start_ms: i64, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms: start_ms + 1000,
            text: text.into(),
            confidence: None,
            no_speech_prob: None,
        }
    }

    fn start(segments: &[Segment], query: &str) -> Option<i64> {
        segment(segments, query).map(|s| s.start_ms)
    }

    #[test]
    fn the_segment_with_the_most_words_wins() {
        let segments = [
            at(0, "I understand therapy"),
            at(4000, "as a concept, I understand therapy"),
            at(9000, "a concept"),
        ];
        assert_eq!(start(&segments, "therapy concept"), Some(4000));
    }

    #[test]
    fn a_tie_goes_to_the_first() {
        let segments = [at(0, "the geese"), at(2000, "more geese")];
        assert_eq!(start(&segments, "geese"), Some(0));
    }

    #[test]
    fn words_match_as_prefixes_and_ignore_case() {
        let segments = [at(0, "nothing here"), at(3000, "Canadian Geese!")];
        assert_eq!(start(&segments, "canad gee"), Some(3000));
    }

    #[test]
    fn a_word_with_an_apostrophe_matches_its_parts() {
        let segments = [at(0, "do it"), at(1500, "don't do it")];
        assert_eq!(start(&segments, "don't"), Some(1500));
    }

    #[test]
    fn tag_terms_do_not_count() {
        let segments = [at(0, "reaction"), at(1000, "cheese")];
        assert_eq!(start(&segments, "tag:reaction cheese"), Some(1000));
    }

    #[test]
    fn no_segment_holds_the_words() {
        assert_eq!(start(&[at(0, "hello")], "goodbye"), None);
    }
}
