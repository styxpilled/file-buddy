//! Where the database, the thumbnail cache and the configuration file are.
//! The FFmpeg executables are found by `ffmpeg::path_of`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const CONFIG_FILE: &str = "file-buddy.toml";
pub const DATABASE_FILE: &str = "file-buddy.db";
pub const THUMBNAIL_DIR: &str = "thumbnails";
pub const MODEL_DIR: &str = "models";
pub const WHISPER_MODEL: &str = "ggml-large-v3-turbo.bin";
pub const CLIP_MODEL: &str = "mobileclip2-s2";

/// The directory that holds the executable. Both the configuration file and the
/// default data directory sit next to it.
pub fn executable_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("read the path of the executable")?;
    Ok(exe
        .parent()
        .context("the executable has no parent directory")?
        .to_path_buf())
}

/// The database and the thumbnail cache. `FILE_BUDDY_DATA` moves both.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("FILE_BUDDY_DATA") {
        return Ok(PathBuf::from(dir));
    }
    executable_dir()
}

pub fn config_path() -> Result<PathBuf> {
    Ok(executable_dir()?.join(CONFIG_FILE))
}

pub fn database_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(DATABASE_FILE))
}

pub fn thumbnail_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join(THUMBNAIL_DIR))
}

/// The speech model. The user supplies it in `models` next to the executable. A
/// development build also looks in `models` at the repository root.
pub fn whisper_model() -> Result<PathBuf> {
    model(WHISPER_MODEL, "speech model", Path::is_file)
}

/// The directory of the image and text model, with its ONNX files, tokenizer
/// and configuration. Found the same way as the speech model.
pub fn clip_model() -> Result<PathBuf> {
    model(CLIP_MODEL, "image model", |dir| {
        dir.join("open_clip_config.json").is_file()
    })
}

fn model(name: &str, what: &str, exists: impl Fn(&Path) -> bool) -> Result<PathBuf> {
    let beside_exe = executable_dir()?.join(MODEL_DIR).join(name);
    let candidates = [
        beside_exe.clone(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(MODEL_DIR)
            .join(name),
    ];
    candidates
        .into_iter()
        .find(|path| exists(path))
        .with_context(|| format!("the {what} is missing. Put it at {}", beside_exe.display()))
}

/// The thumbnail of one file, in a directory named after the first two
/// characters of the hash so no single directory holds every file.
pub fn thumbnail_path(cache: &Path, hash: &str) -> PathBuf {
    cache.join(&hash[..2]).join(format!("{hash}.jpg"))
}

/// The smaller thumbnail a tile shows, beside the full one.
pub fn tile_path(cache: &Path, hash: &str) -> PathBuf {
    cache.join(&hash[..2]).join(format!("{hash}.tile.jpg"))
}

/// A copy FFmpeg can decode, of a file it cannot. See `ffmpeg::webp`.
pub fn proxy_path(cache: &Path, hash: &str) -> PathBuf {
    cache.join(&hash[..2]).join(format!("{hash}.proxy.mp4"))
}

/// What to play: the proxy if the file has one, else the file.
pub fn playable(cache: &Path, hash: &str, path: &Path) -> PathBuf {
    let proxy = proxy_path(cache, hash);
    if proxy.is_file() {
        proxy
    } else {
        path.to_path_buf()
    }
}
