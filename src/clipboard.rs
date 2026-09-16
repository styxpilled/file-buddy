//! The Windows clipboard. GPUI cannot do this part: its `ClipboardEntry` holds
//! a string or an image, never a file.
//!
//! One copy writes two formats. `CF_HDROP` is the one an application such as
//! Explorer or Discord reads as a file to upload. `CF_UNICODETEXT` is the path,
//! for a text field, a terminal or a chat message. Writing only the first makes
//! the copy look broken everywhere text is expected, and only the second makes
//! it look broken everywhere a file is expected.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use clipboard_win::options::NoClear;
use clipboard_win::{Clipboard, raw};

/// Tries at opening the clipboard. Only one process may hold it at a time, and
/// another one often holds it for a few milliseconds after a paste.
const ATTEMPTS: usize = 10;

/// Put one file on the clipboard, as a file and as its path.
pub fn copy_file(path: &Path) -> Result<()> {
    let path = dunce::canonicalize(path).expect("cannot canonicalize path");
    let path = path.to_str().expect("cannot convert copy path to str");

    let _clipboard = Clipboard::new_attempts(ATTEMPTS)
        .map_err(|e| anyhow!("another application is holding the clipboard: {e}"))?;

    // Each writer must leave what the other wrote, so the clipboard is emptied
    // once here and both writes use the form that does not empty it again.
    raw::empty().map_err(|e| anyhow!("cannot empty the clipboard: {e}"))?;
    raw::set_string_with(&path, NoClear).map_err(|e| anyhow!("cannot write the path: {e}"))?;
    raw::set_file_list_with(&[&path], NoClear)
        .map_err(|e| anyhow!("cannot write the file: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipboard_win::Getter;
    use clipboard_win::formats::{FileList, Unicode};
    use std::path::PathBuf;

    /// Reads both formats back, because a copy that writes only one of them
    /// looks right until it is pasted somewhere that wants the other. This
    /// replaces whatever is on the clipboard while the test runs.
    #[test]
    fn a_copied_file_is_on_the_clipboard_as_a_file_and_as_a_path() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        // As a caller with a path out of the config file would give it.
        copy_file(Path::new(&path.to_str().unwrap().replace('\\', "/"))).unwrap();

        let _clipboard = Clipboard::new_attempts(ATTEMPTS).unwrap();

        let mut files: Vec<PathBuf> = Vec::new();
        FileList.read_clipboard(&mut files).unwrap();
        assert_eq!(files, vec![path.clone()]);

        let mut text = String::new();
        Unicode.read_clipboard(&mut text).unwrap();
        assert_eq!(text, path.to_str().unwrap());
    }
}
