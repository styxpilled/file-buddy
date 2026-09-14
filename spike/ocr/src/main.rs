//! Spike for issue 01. Runs Windows.Media.Ocr over extracted frames and prints
//! the raw text per file. Throwaway.

use std::collections::BTreeSet;
use std::future::IntoFuture;
use std::path::{Path, PathBuf};

use windows::Globalization::Language;
use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};
use windows::core::HSTRING;

fn engine() -> windows::core::Result<OcrEngine> {
    match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(e) => Ok(e),
        Err(_) => OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from(
            "en-US",
        ))?),
    }
}

fn ocr_frame(engine: &OcrEngine, path: &Path) -> windows::core::Result<Vec<String>> {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();

    let file = pollster::block_on(StorageFile::GetFileFromPathAsync(&HSTRING::from(abs))?.into_future())?;
    let stream = pollster::block_on(file.OpenAsync(FileAccessMode::Read)?.into_future())?;
    let decoder = pollster::block_on(BitmapDecoder::CreateAsync(&stream)?.into_future())?;
    let bitmap = pollster::block_on(decoder.GetSoftwareBitmapAsync()?.into_future())?;

    let result = pollster::block_on(engine.RecognizeAsync(&bitmap)?.into_future())?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let text = line.Text()?.to_string();
        if !text.trim().is_empty() {
            lines.push(text);
        }
    }
    Ok(lines)
}

fn clip_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .expect("frames root")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn main() -> windows::core::Result<()> {
    let root = PathBuf::from(std::env::args().nth(1).expect("usage: ocr-spike <frames dir>"));

    let langs: Vec<String> = OcrEngine::AvailableRecognizerLanguages()?
        .into_iter()
        .filter_map(|l| l.LanguageTag().ok())
        .map(|t| t.to_string())
        .collect();
    println!("available OCR languages: {}", langs.join(", "));

    let engine = engine()?;
    println!(
        "engine language: {}\n",
        engine.RecognizerLanguage()?.LanguageTag()?
    );

    for category in ["text", "no_text"] {
        let cat_dir = root.join(category);
        if !cat_dir.is_dir() {
            continue;
        }
        println!("\n{:=^78}", format!(" {category} "));

        for clip in clip_dirs(&cat_dir) {
            let name = clip.file_name().unwrap().to_string_lossy().to_string();
            let mut frames: Vec<PathBuf> = std::fs::read_dir(&clip)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "png"))
                .collect();
            frames.sort();

            let started = std::time::Instant::now();
            let mut per_frame = Vec::new();
            let mut unique: BTreeSet<String> = BTreeSet::new();
            for frame in &frames {
                let lines = ocr_frame(&engine, frame)?;
                for l in &lines {
                    unique.insert(l.to_lowercase());
                }
                per_frame.push((frame.file_name().unwrap().to_string_lossy().to_string(), lines));
            }
            let elapsed = started.elapsed();

            println!("\n--- {name}");
            println!(
                "    {} frames, {:?} total, {:?}/frame, {} unique lines",
                frames.len(),
                elapsed,
                elapsed.checked_div(frames.len().max(1) as u32).unwrap_or_default(),
                unique.len()
            );
            for (frame, lines) in &per_frame {
                if lines.is_empty() {
                    println!("    [{frame}] -");
                } else {
                    for l in lines {
                        println!("    [{frame}] {l}");
                    }
                }
            }
        }
    }
    Ok(())
}
