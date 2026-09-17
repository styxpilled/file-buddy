//! Text the recognizer reads in many files that nobody searches for. A new
//! rule goes here.
//!
//! This runs where the search index is built, not where a file is read, so the
//! `ocr` table keeps every line the recognizer found. A change to a rule here
//! costs an index rebuild and not another pass of frame extraction.

/// A line that holds fewer letters than this, once the watermark is taken out
/// of it, says nothing on its own.
const MIN_LETTERS_LEFT: usize = 3;

/// Whether the line is only a platform watermark.
///
/// TikTok is the first: the word mark and the handle under it are in nearly
/// every clip from the platform. The line goes only when the watermark is all
/// it says, so a caption that names a handle or the platform stays.
pub fn is_noise(line: &str) -> bool {
    let mut watermark = false;
    let mut handle = false;
    let mut rest = 0;
    for token in line.split_whitespace() {
        let token = token.to_lowercase();
        // The token after a lone `@` is the handle that `@` introduces.
        if std::mem::take(&mut handle) {
            continue;
        }
        if is_word_mark(&token) || token.starts_with('@') {
            watermark = true;
            handle = token == "@";
            continue;
        }
        rest += token.chars().filter(|c| c.is_alphabetic()).count();
    }
    watermark && rest < MIN_LETTERS_LEFT
}

/// The word mark, as the recognizer spells it. It reads a zero for the letter
/// `o` often enough to be worth covering. A mangling that loses a letter, such
/// as `ikTok`, is not covered: that needs a likeness and not a word.
fn is_word_mark(token: &str) -> bool {
    token.replace('0', "o").contains("tiktok")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_watermark_is_noise() {
        for line in [
            "TikTok",
            "TikT0k",
            "@ mangoslug",
            "@mangoslug",
            "TikTok @ realistmarbles",
            "Q @ ladymisskay_",
        ] {
            assert!(is_noise(line), "{line}");
        }
    }

    #[test]
    fn a_caption_that_names_a_handle_or_the_platform_is_not() {
        for line in [
            "@mangoslug is the best",
            "i posted this on tiktok",
            "Meet the silky anteater",
        ] {
            assert!(!is_noise(line), "{line}");
        }
    }
}
