//! What the user typed becomes an FTS5 expression. Every term is a prefix
//! term, because the search runs on every keystroke. A query that starts with
//! `~` searches only by what the frames show.

/// The rest of a query that starts with `~`.
pub fn visual_only(query: &str) -> Option<&str> {
    query.trim_start().strip_prefix('~').map(str::trim)
}

/// The words the text encoder reads: the query without the `~`, and without
/// the terms limited to a column, which say nothing about a frame.
pub fn visual_text(query: &str) -> Option<String> {
    let query = visual_only(query).unwrap_or(query);
    let words: Vec<&str> = query
        .split_whitespace()
        .filter(|word| !matches!(word.split_once(':'), Some(("tag" | "tags", _))))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// The expression for a query, or `None` when there is nothing to search for.
pub fn build_match(query: &str) -> Option<String> {
    let mut terms = Vec::new();
    for word in query.split_whitespace() {
        let (column, text) = match word.split_once(':') {
            Some(("tag" | "tags", rest)) => ("tags:", rest),
            _ => ("", word),
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        // Quoting makes every character safe to FTS5. A quote inside is doubled.
        terms.push(format!("{column}\"{}\"*", text.replace('"', "\"\"")));
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_term_is_a_prefix_term() {
        assert_eq!(build_match("dale coop").unwrap(), r#""dale"* "coop"*"#);
    }

    #[test]
    fn the_tag_operator_limits_a_term_to_one_column() {
        assert_eq!(
            build_match("tag:reaction cheese").unwrap(),
            r#"tags:"reaction"* "cheese"*"#
        );
        assert_eq!(build_match("tags:reaction").unwrap(), r#"tags:"reaction"*"#);
    }

    #[test]
    fn an_empty_query_has_no_expression() {
        assert!(build_match("").is_none());
        assert!(build_match("   ").is_none());
        assert!(build_match("tag:").is_none());
    }

    #[test]
    fn a_tilde_at_the_start_searches_only_by_the_frames() {
        assert_eq!(visual_only("~cat in a box"), Some("cat in a box"));
        assert_eq!(visual_only("  ~ cat "), Some("cat"));
        assert_eq!(visual_only("~"), Some(""));
        assert_eq!(visual_only("cat ~box"), None);
    }

    #[test]
    fn the_text_encoder_reads_the_words_without_the_operators() {
        assert_eq!(visual_text("~cat in a box").unwrap(), "cat in a box");
        assert_eq!(visual_text("tag:reaction cat").unwrap(), "cat");
        assert!(visual_text("tag:reaction").is_none());
        assert!(visual_text("~").is_none());
    }

    #[test]
    fn fts5_syntax_in_the_query_is_text_not_syntax() {
        assert_eq!(build_match(r#"a"b"#).unwrap(), r#""a""b"*"#);
        assert_eq!(build_match("NOT").unwrap(), r#""NOT"*"#);
        assert_eq!(build_match("a*").unwrap(), r#""a*"*"#);
    }
}
