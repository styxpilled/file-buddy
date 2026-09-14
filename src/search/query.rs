//! What the user typed becomes an FTS5 expression. Every term is a prefix
//! term, because the search runs on every keystroke.

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
    fn fts5_syntax_in_the_query_is_text_not_syntax() {
        assert_eq!(build_match(r#"a"b"#).unwrap(), r#""a""b"*"#);
        assert_eq!(build_match("NOT").unwrap(), r#""NOT"*"#);
        assert_eq!(build_match("a*").unwrap(), r#""a*"*"#);
    }
}
