//! Where the database, the thumbnail cache and the configuration file are.
//! The FFmpeg executables are found by `ffmpeg::path_of`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const CONFIG_FILE: &str = "file-buddy.toml";
pub const DATABASE_FILE: &str = "file-buddy.db";
pub const THUMBNAIL_DIR: &str = "thumbnails";

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

/// The thumbnail of one file, in a directory named after the first two
/// characters of the hash so no single directory holds every file.
pub fn thumbnail_path(cache: &Path, hash: &str) -> PathBuf {
    cache.join(&hash[..2]).join(format!("{hash}.jpg"))
}
