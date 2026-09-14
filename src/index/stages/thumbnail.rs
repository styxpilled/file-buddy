//! The still frame that the grid shows. One per content hash, so duplicates and
//! renames share it.

use std::path::Path;

use anyhow::Result;

use crate::ffmpeg::thumbnail;

pub fn run(hash: &str, source: &Path, cache: &Path) -> Result<()> {
    thumbnail::generate(source, hash, cache)?;
    Ok(())
}
