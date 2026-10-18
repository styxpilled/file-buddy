//! The Windows clipboard. GPUI cannot do this part: its `ClipboardEntry` holds
//! a string or an image, never a file.
//!
//! One copy writes two formats. `CF_HDROP` is the one an application such as
//! Explorer or Discord reads as a file to upload. `CF_UNICODETEXT` is the path,
//! for a text field, a terminal or a chat message. Writing only the first makes
//! the copy look broken everywhere text is expected, and only the second makes
//! it look broken everywhere a file is expected.
//!
//! A screenshot is an image with no file, as `PNG` and as `CF_DIB`. A paste
//! reads the same three formats back.

use std::path::{Path, PathBuf};

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

/// What a paste found on the clipboard.
pub enum Pasted {
    Files(Vec<PathBuf>),
    /// The bytes of a PNG file.
    Image(Vec<u8>),
    Nothing,
}

/// Files first, as Explorer copies them. Then an image: the registered `PNG`
/// format keeps alpha, and `CF_DIB` is there for applications that write
/// only that.
pub fn paste() -> Result<Pasted> {
    let _clipboard = Clipboard::new_attempts(ATTEMPTS)
        .map_err(|e| anyhow!("another application is holding the clipboard: {e}"))?;
    if raw::is_format_avail(formats::CF_HDROP) {
        let mut files = Vec::new();
        raw::get_file_list_path(&mut files).map_err(|e| anyhow!("cannot read the files: {e}"))?;
        return Ok(Pasted::Files(files));
    }
    if let Some(png_format) = raw::register_format("PNG")
        && raw::is_format_avail(png_format.get())
    {
        let mut png = Vec::new();
        raw::get_vec(png_format.get(), &mut png)
            .map_err(|e| anyhow!("cannot read the PNG: {e}"))?;
        if png.starts_with(b"\x89PNG") {
            return Ok(Pasted::Image(png));
        }
    }
    if raw::is_format_avail(formats::CF_DIB) {
        let mut bytes = Vec::new();
        raw::get_vec(formats::CF_DIB, &mut bytes)
            .map_err(|e| anyhow!("cannot read the bitmap: {e}"))?;
        let mut png = Vec::new();
        from_dib(&bytes)?
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| anyhow!("cannot write the PNG: {e}"))?;
        return Ok(Pasted::Image(png));
    }
    Ok(Pasted::Nothing)
}

/// The image of a `CF_DIB`: 24 or 32 bits a pixel, uncompressed or with bit
/// fields, top-down or bottom-up.
fn from_dib(dib: &[u8]) -> Result<image::RgbaImage> {
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    let u32_at = |at: usize| -> Result<u32> {
        dib.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| anyhow!("the bitmap is cut short"))
    };
    let header = u32_at(0)? as usize;
    let width = u32_at(4)? as i32;
    let height = u32_at(8)? as i32;
    let bits = u32_at(12)? >> 16;
    let compression = u32_at(16)?;
    let colors = u32_at(32)? as usize;
    if width <= 0 || height == 0 || header < 40 {
        return Err(anyhow!("the bitmap has no size"));
    }
    let masks = match (compression, bits) {
        (BI_RGB, 24 | 32) => [0xff_0000, 0xff00, 0xff, 0xff00_0000],
        (BI_BITFIELDS, 32) => {
            // The masks come at byte 40: after a plain info header, inside a
            // V4 or V5 one, which also has an alpha mask.
            let alpha = if header >= 56 { u32_at(52)? } else { 0 };
            [u32_at(40)?, u32_at(44)?, u32_at(48)?, alpha]
        }
        _ => return Err(anyhow!("a bitmap of {bits} bits is not supported")),
    };
    let extra = if compression == BI_BITFIELDS && header == 40 { 12 } else { 0 };
    let start = header + extra + colors * 4;
    let (width, rows) = (width as u32, height.unsigned_abs());
    let bytes = (bits / 8) as usize;
    let stride = (width as usize * bits as usize).div_ceil(32) * 4;
    if dib.len() < start + stride * rows as usize {
        return Err(anyhow!("the bitmap is cut short"));
    }
    let channel = |pixel: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 0;
        }
        let value = (pixel & mask) >> mask.trailing_zeros();
        let max = mask >> mask.trailing_zeros();
        (value * 255 / max) as u8
    };
    let mut image = image::RgbaImage::new(width, rows);
    for y in 0..rows {
        let row = if height > 0 { rows - 1 - y } else { y };
        let line = &dib[start + row as usize * stride..];
        for x in 0..width {
            let at = x as usize * bytes;
            let mut word = [0u8; 4];
            word[..bytes].copy_from_slice(&line[at..at + bytes]);
            let pixel = u32::from_le_bytes(word);
            let [r, g, b, a] = masks.map(|mask| channel(pixel, mask));
            image.put_pixel(x, y, image::Rgba([r, g, b, a]));
        }
    }
    // Many applications leave the alpha byte at zero, and mean opaque.
    if bits == 24 || image.pixels().all(|pixel| pixel.0[3] == 0) {
        for pixel in image.pixels_mut() {
            pixel.0[3] = 255;
        }
    }
    Ok(image)
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
    #[test]
    fn a_dib_reads_back_as_the_image() {
        let mut rgba = image::RgbaImage::new(3, 2);
        rgba.put_pixel(0, 0, image::Rgba([1, 2, 3, 4]));
        rgba.put_pixel(2, 1, image::Rgba([5, 6, 7, 8]));
        assert_eq!(from_dib(&dib(&rgba)).unwrap(), rgba);
    }

    #[test]
    fn a_24_bit_dib_is_opaque_and_its_rows_are_padded() {
        // One row of three pixels, 9 bytes padded to 12, top-down.
        let mut bytes = Vec::new();
        bytes.extend(40u32.to_le_bytes());
        bytes.extend(3i32.to_le_bytes());
        bytes.extend((-1i32).to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(24u16.to_le_bytes());
        bytes.extend([0; 24]);
        bytes.extend([3, 2, 1, 6, 5, 4, 9, 8, 7, 0, 0, 0]);
        let image = from_dib(&bytes).unwrap();
        let pixels: Vec<_> = image.pixels().map(|p| p.0).collect();
        assert_eq!(pixels, [[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255]]);
    }

    #[test]
    fn a_32_bit_dib_with_no_alpha_is_opaque() {
        let rgba = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 0]));
        let image = from_dib(&dib(&rgba)).unwrap();
        assert!(image.pixels().all(|p| p.0 == [10, 20, 30, 255]));
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
