//! One thumbnail per content hash, from the FFmpeg `thumbnail` filter, which
//! picks a representative frame and so avoids a black opening frame. Video,
//! animation and still image all take the same command.

use std::path::Path;

use anyhow::{Context, Result, bail};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

use crate::ffmpeg::{self, Tool};
use crate::paths;

/// The longest edge of a thumbnail, in pixels.
const MAX_EDGE: u32 = 480;
/// How many frames the filter compares before it picks one.
const CANDIDATES: u32 = 100;
/// The box a tile thumbnail fits in: twice the picture in a tile, so it stays
/// sharp at 200 % display scale.
const TILE_WIDTH: u32 = 432;
const TILE_HEIGHT: u32 = 240;
const TILE_QUALITY: u8 = 85;

/// Write the thumbnail of one file into the cache.
pub fn generate(source: &Path, hash: &str, cache: &Path) -> Result<()> {
    let destination = paths::thumbnail_path(cache, hash);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let filter = format!(
        "thumbnail=n={CANDIDATES},scale=w='min({MAX_EDGE},iw)':h='min({MAX_EDGE},ih)':\
         force_original_aspect_ratio=decrease:force_divisible_by=2:flags=lanczos"
    );
    let partial = destination.with_extension("part.jpg");
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .arg("-i")
        .arg(source)
        .args(["-map", "0:v:0", "-an", "-sn", "-dn"])
        .args(["-vf", &filter, "-frames:v", "1", "-q:v", "4"])
        .arg(&partial);
    if let Err(e) = ffmpeg::output(command) {
        std::fs::remove_file(&partial).ok();
        return Err(e.context("thumbnail"));
    }
    // FFmpeg can exit zero and write nothing when the filter gets no frame.
    if std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0) == 0 {
        std::fs::remove_file(&partial).ok();
        bail!("thumbnail failed: ffmpeg produced no frame");
    }
    // Renamed last, so a partial file is never mistaken for a finished one.
    std::fs::rename(&partial, &destination)
        .with_context(|| format!("move the thumbnail to {}", destination.display()))?;
    // The window makes a missing tile thumbnail when it first shows the tile.
    if let Err(e) = generate_tile(hash, cache) {
        log::warn!("tile thumbnail of {hash}: {e:#}");
    }
    Ok(())
}

/// Write the tile thumbnail from the full one. GPUI keeps an image decoded at
/// its full size, so a tile that showed the full thumbnail held up to four
/// times the pixels it draws.
pub fn generate_tile(hash: &str, cache: &Path) -> Result<()> {
    let source = paths::thumbnail_path(cache, hash);
    let destination = paths::tile_path(cache, hash);
    let full = image::open(&source).with_context(|| format!("read {}", source.display()))?;
    let tile = if full.width() > TILE_WIDTH || full.height() > TILE_HEIGHT {
        full.resize(TILE_WIDTH, TILE_HEIGHT, FilterType::Lanczos3)
    } else {
        full
    };
    let partial = destination.with_extension("part.jpg");
    let file =
        std::fs::File::create(&partial).with_context(|| format!("create {}", partial.display()))?;
    let written = JpegEncoder::new_with_quality(std::io::BufWriter::new(file), TILE_QUALITY)
        .encode_image(&tile.to_rgb8());
    if let Err(e) = written {
        std::fs::remove_file(&partial).ok();
        return Err(e).context("encode the tile thumbnail");
    }
    std::fs::rename(&partial, &destination)
        .with_context(|| format!("move the tile thumbnail to {}", destination.display()))
}

/// Whether the cache already holds this thumbnail.
pub fn exists(hash: &str, cache: &Path) -> bool {
    paths::thumbnail_path(cache, hash).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_full(cache: &Path, hash: &str, width: u32, height: u32) {
        let path = paths::thumbnail_path(cache, hash);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbImage::new(width, height).save(&path).unwrap();
    }

    #[test]
    fn a_tile_thumbnail_fits_the_tile_box_and_keeps_its_shape() {
        let cache = tempfile::tempdir().unwrap();
        for (hash, full, tile) in [
            ("aa01", (480, 270), (427, 240)),
            ("aa02", (270, 480), (135, 240)),
            ("aa03", (200, 100), (200, 100)),
        ] {
            write_full(cache.path(), hash, full.0, full.1);
            generate_tile(hash, cache.path()).unwrap();
            let made = image::open(paths::tile_path(cache.path(), hash)).unwrap();
            assert_eq!((made.width(), made.height()), tile, "{hash}");
        }
    }
}
