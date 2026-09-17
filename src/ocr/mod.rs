//! On-screen text. `Engine` is the whole surface the pipeline sees, so another
//! recognizer can replace `Windows.Media.Ocr` without a change to the stage.

pub mod noise;
mod windows;

use std::path::Path;

use anyhow::Result;

/// A recognizer. Lines, not one block of text: the line structure is what
/// separates a caption from the chart noise beside it.
pub trait Engine: Send {
    /// The lines of text in one image, in the order the recognizer read them.
    fn read(&self, image: &Path) -> Result<Vec<String>>;
}

/// The recognizer of this machine.
pub fn engine() -> Result<Box<dyn Engine>> {
    Ok(Box::new(windows::WindowsOcr::new()?))
}
