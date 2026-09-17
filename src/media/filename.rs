//! What the application does to a file name: the split for the search index,
//! the cleanup of a name the user typed, and the test for a name that says
//! nothing.
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

/// Characters that Windows refuses in a file name.
const FORBIDDEN: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Device names that Windows reserves, with or without an extension.
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// A typed stem made safe for Windows. Forbidden and control characters are
/// dropped, runs of spaces become one, and trailing dots and spaces go, because
/// Windows strips them silently. An empty or reserved name is an error.
pub fn sanitize(typed: &str) -> Result<String, String> {
    let kept: String = typed
        .chars()
        .filter(|c| !FORBIDDEN.contains(c) && !c.is_control())
        .collect();
    let name = kept
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', ' '])
        .to_string();
    if name.is_empty() {
        return Err("the name is empty".into());
    }
    let device = name.split('.').next().unwrap_or_default().to_lowercase();
    if RESERVED.contains(&device.as_str()) {
        return Err(format!("{name} is a name Windows reserves"));
    }
    Ok(name)
}

/// A stem that a person did not choose: it holds a `_`, a `-`, a bracket, a
/// character outside ASCII, or a number longer than 4 digits.
pub fn is_poor(stem: &str) -> bool {
    let mut digits = 0;
    for c in stem.chars() {
        if matches!(c, '_' | '-' | '(' | ')' | '[' | ']' | '{' | '}') || !c.is_ascii() {
            return true;
        }
        digits = if c.is_ascii_digit() { digits + 1 } else { 0 };
        if digits > 4 {
            return true;
        }
    }
    false
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
    fn sanitize_drops_what_windows_forbids() {
        assert_eq!(sanitize("what? a <clip>").unwrap(), "what a clip");
        assert_eq!(sanitize("a/b\\c:d|e*f\"g").unwrap(), "abcdefg");
        assert_eq!(sanitize("  two   spaces\t").unwrap(), "two spaces");
        assert_eq!(sanitize("trailing dots...").unwrap(), "trailing dots");
    }

    #[test]
    fn sanitize_refuses_empty_and_reserved_names() {
        assert!(sanitize("").is_err());
        assert!(sanitize("?*. ").is_err());
        assert!(sanitize("CON").is_err());
        assert!(sanitize("nul.backup").is_err());
        assert_eq!(sanitize("console").unwrap(), "console");
    }

    #[test]
    fn poor_names() {
        for stem in [
            "IMG_2041",
            "video-final",
            "clip (2)",
            "[meme]",
            "café",
            "20240914",
            "1726329600123",
        ] {
            assert!(is_poor(stem), "{stem}");
        }
        for stem in [
            "my honest reaction",
            "prod moment",
            "top 2024 goals",
            "clip 1234",
        ] {
            assert!(!is_poor(stem), "{stem}");
        }
    }

    #[test]
    fn handles_empty_and_separator_only_names() {
        assert!(tokens("").is_empty());
        assert!(tokens("___").is_empty());
    }
}
