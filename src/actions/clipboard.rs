//! The Windows clipboard. GPUI cannot do this part: its `ClipboardEntry` holds
//! a string or an image, never a file.
//!
//! One copy writes two formats. `CF_HDROP` is the one an application such as
//! Explorer or Discord reads as a file to upload. `CF_UNICODETEXT` is the path,
//! for a text field, a terminal or a chat message. Writing only the first makes
//! the copy look broken everywhere text is expected, and only the second makes
//! it look broken everywhere a file is expected.
//!
//! A screenshot is an image with no file, as `PNG` and as `CF_DIB`.

use std::path::Path;

use anyhow::{Result, anyhow};
use clipboard_win::options::NoClear;
use clipboard_win::{Clipboard, formats, raw};

/// Tries at opening the clipboard. Only one process may hold it at a time, and
/// another one often holds it for a few milliseconds after a paste.
const ATTEMPTS: usize = 10;

/// Put one file on the clipboard, as a file and as its path.
pub fn copy_file(path: &Path) -> Result<()> {
    let path = dunce::canonicalize(path).map_err(|e| anyhow!("cannot canonicalize path: {e}"))?;
    let Some(path) = path.to_str() else {
        return Err(anyhow!("cannot convert copy path to str"));
    };

    let _clipboard = Clipboard::new_attempts(ATTEMPTS)
        .map_err(|e| anyhow!("another application is holding the clipboard: {e}"))?;

    // Each writer must leave what the other wrote, so the clipboard is emptied
    // once here and both writes use the form that does not empty it again.
    raw::empty().map_err(|e| anyhow!("cannot empty the clipboard: {e}"))?;
    raw::set_string_with(path, NoClear).map_err(|e| anyhow!("cannot write the path: {e}"))?;
    raw::set_file_list_with(&[&path], NoClear)
        .map_err(|e| anyhow!("cannot write the file: {e}"))?;

    Ok(())
}

/// Put an image on the clipboard as the registered `PNG` format, which keeps
/// alpha, and as `CF_DIB`, for applications that read only that. Returns the
/// size of the image.
pub fn copy_image(png: &[u8]) -> Result<(u32, u32)> {
    let rgba = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| anyhow!("cannot read the frame: {e}"))?
        .into_rgba8();
    let size = rgba.dimensions();
    let dib = dib(&rgba);
    let png_format =
        raw::register_format("PNG").ok_or_else(|| anyhow!("cannot register the PNG format"))?;

    let _clipboard = Clipboard::new_attempts(ATTEMPTS)
        .map_err(|e| anyhow!("another application is holding the clipboard: {e}"))?;
    raw::empty().map_err(|e| anyhow!("cannot empty the clipboard: {e}"))?;
    raw::set_without_clear(png_format.get(), png)
        .map_err(|e| anyhow!("cannot write the PNG: {e}"))?;
    raw::set_without_clear(formats::CF_DIB, &dib)
        .map_err(|e| anyhow!("cannot write the bitmap: {e}"))?;
    Ok(size)
}

/// A `BITMAPINFOHEADER` and 32-bit BGRA rows, bottom row first.
fn dib(rgba: &image::RgbaImage) -> Vec<u8> {
    const HEADER: u32 = 40;
    let (width, height) = rgba.dimensions();
    let pixels = width * height * 4;
    let mut dib = Vec::with_capacity((HEADER + pixels) as usize);
    dib.extend(HEADER.to_le_bytes());
    dib.extend((width as i32).to_le_bytes());
    dib.extend((height as i32).to_le_bytes());
    dib.extend(1u16.to_le_bytes());
    dib.extend(32u16.to_le_bytes());
    // BI_RGB, the image size, the resolution and the palette.
    dib.extend(0u32.to_le_bytes());
    dib.extend(pixels.to_le_bytes());
    dib.extend([0; 16]);
    for row in rgba.rows().rev() {
        for pixel in row {
            let [r, g, b, a] = pixel.0;
            dib.extend([b, g, r, a]);
        }
    }
    dib
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dib_is_bottom_up_bgra_after_its_header() {
        let mut rgba = image::RgbaImage::new(2, 2);
        rgba.put_pixel(0, 0, image::Rgba([1, 2, 3, 4]));
        rgba.put_pixel(1, 1, image::Rgba([5, 6, 7, 8]));
        let dib = dib(&rgba);
        assert_eq!(dib.len(), 40 + 16);
        assert_eq!(&dib[..4], &40u32.to_le_bytes());
        assert_eq!(&dib[4..12], &[2, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(&dib[14..16], &32u16.to_le_bytes());
        // The bottom row comes first.
        assert_eq!(&dib[40..48], &[0, 0, 0, 0, 7, 6, 5, 8]);
        assert_eq!(&dib[48..56], &[3, 2, 1, 4, 0, 0, 0, 0]);
    }
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
