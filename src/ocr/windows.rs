//! `Windows.Media.Ocr`. It ships with the system, needs no model file and no
//! second process, and issue 01 measured it as accurate enough.
//!
//! Every call is asynchronous and none of them blocks, so each one is driven to
//! its end here. The recognizer itself is cheap, about 20 ms for a frame.

use std::future::IntoFuture;
use std::path::Path;

use anyhow::{Context, Result};
use windows::Globalization::Language;
use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};
use windows::core::HSTRING;

use crate::ocr::Engine;

pub struct WindowsOcr {
    engine: OcrEngine,
}

impl WindowsOcr {
    /// The recognizer of the user's own languages, or English when the user has
    /// none that recognizes text.
    pub fn new() -> Result<Self> {
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().or_else(|_| {
            OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from("en-US"))?)
        });
        Ok(Self {
            engine: engine.context("no OCR language is installed")?,
        })
    }
}

impl Engine for WindowsOcr {
    fn read(&self, image: &Path) -> Result<Vec<String>> {
        // `StorageFile` takes a full path and rejects the verbatim form that
        // `canonicalize` gives, which is what `dunce` is here for.
        let path = dunce::canonicalize(image)
            .with_context(|| format!("read {}", image.display()))?
            .to_string_lossy()
            .into_owned();

        let file = block_on(StorageFile::GetFileFromPathAsync(&HSTRING::from(path))?)?;
        let stream = block_on(file.OpenAsync(FileAccessMode::Read)?)?;
        let decoder = block_on(BitmapDecoder::CreateAsync(&stream)?)?;
        let bitmap = block_on(decoder.GetSoftwareBitmapAsync()?)?;

        let result = block_on(self.engine.RecognizeAsync(&bitmap)?)?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        Ok(lines)
    }
}

/// Wait for one WinRT operation. The stage already runs on its own worker
/// thread, so blocking it costs nothing.
fn block_on<F: IntoFuture>(operation: F) -> F::Output {
    pollster::block_on(operation.into_future())
}
