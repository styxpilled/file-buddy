//! What the application does to a file name. Today that is the split for the
//! search index. Issue 08 adds the sanitization that the rename walk needs.
//!
//! A wrong guess here makes a file impossible to find with no error, so the
//! split never removes a token. `1080p` stays.

/// Characters that separate words in a file name.
const SEPARATORS: [char; 8] = ['_', '-', '.', '(', ')', '[', ']', ' '];

/// Lowercased tokens of a file name, split on the separators and on camelCase.
pub fn tokens(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in name.split(|c| SEPARATORS.contains(&c)) {
        for part in split_camel_case(word) {
            if !part.is_empty() {
                out.push(part.to_lowercase());
            }
        }
    }
    out
}

/// The indexed form of a file name: the tokens joined by a space.
pub fn index_form(name: &str) -> String {
    tokens(name).join(" ")
}

/// `fooBar` becomes `foo Bar`, `HTTPServer` becomes `HTTP Server`. A digit never
/// starts a new part, so `1080p` and `h264` stay whole.
fn split_camel_case(word: &str) -> Vec<&str> {
    let chars: Vec<char> = word.chars().collect();
    let mut parts = Vec::new();
    let mut byte_start = 0;
    let mut byte_at = Vec::with_capacity(chars.len() + 1);
    let mut offset = 0;
    for c in &chars {
        byte_at.push(offset);
        offset += c.len_utf8();
    }
    byte_at.push(offset);

    for i in 1..chars.len() {
        let previous = chars[i - 1];
        let current = chars[i];
        let next = chars.get(i + 1).copied();
        let lower_to_upper = previous.is_lowercase() && current.is_uppercase();
        let acronym_end = previous.is_uppercase()
            && current.is_uppercase()
            && next.is_some_and(char::is_lowercase);
        if lower_to_upper || acronym_end {
            parts.push(&word[byte_start..byte_at[i]]);
            byte_start = byte_at[i];
        }
    }
    parts.push(&word[byte_start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_separators() {
        assert_eq!(index_form("a few small beers.mp4"), "a few small beers mp4");
        assert_eq!(index_form("clip_01-final.mkv"), "clip 01 final mkv");
        assert_eq!(index_form("shot (2).png"), "shot 2 png");
    }

    #[test]
    fn splits_camel_case() {
        assert_eq!(index_form("myHonestReaction.mp4"), "my honest reaction mp4");
        assert_eq!(index_form("HTTPServerLog"), "http server log");
    }

    #[test]
    fn keeps_noise_tokens_whole() {
        assert_eq!(
            index_form("trailer_1080p_h264.mp4"),
            "trailer 1080p h264 mp4"
        );
        assert_eq!(index_form("Screenshot2026-09-14"), "screenshot2026 09 14");
    }

    #[test]
    fn handles_empty_and_separator_only_names() {
        assert!(tokens("").is_empty());
        assert!(tokens("___").is_empty());
    }
}
