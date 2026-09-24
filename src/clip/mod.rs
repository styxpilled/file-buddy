//! What the frames show, as vectors that a text query can be compared with. The
//! image encoder runs in the `clip` worker process on the GPU, and the text
//! encoder runs in the window on the CPU.
//!
//! ONNX Runtime is loaded at run time from the vendored `onnxruntime/bin`, with
//! its DirectML build.

pub mod image;
pub mod text;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

/// One sampled frame of a file, and what it shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub time_ms: i64,
    /// Normalized, so a dot product is the cosine.
    pub vector: Vec<f32>,
}

/// The parts of `open_clip_config.json` that the encoders need.
#[derive(Debug, Deserialize)]
struct Config {
    model_cfg: ModelConfig,
    preprocess_cfg: PreprocessConfig,
}

#[derive(Debug, Deserialize)]
struct ModelConfig {
    vision_cfg: VisionConfig,
    text_cfg: TextConfig,
}

#[derive(Debug, Deserialize)]
struct VisionConfig {
    image_size: u32,
}

#[derive(Debug, Deserialize)]
struct TextConfig {
    context_length: usize,
}

#[derive(Debug, Deserialize)]
struct PreprocessConfig {
    mean: [f32; 3],
    std: [f32; 3],
}

impl Config {
    fn read(dir: &Path) -> Result<Self> {
        let path = dir.join("open_clip_config.json");
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
    }
}

/// The side of the square frame the image encoder takes.
pub fn image_size(dir: &Path) -> Result<u32> {
    Ok(Config::read(dir)?.model_cfg.vision_cfg.image_size)
}

/// Load ONNX Runtime once per process. DirectML.dll is loaded first by its full
/// path, because Windows 10 has an older one in System32 that would be found
/// before ours.
fn runtime() -> Result<()> {
    static LOADED: OnceLock<Result<(), String>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let dir = runtime_dir();
            ort::util::preload_dylib(dir.join("DirectML.dll"))
                .map_err(|e| format!("load DirectML.dll from {}: {e}", dir.display()))?;
            ort::init_from(dir.join("onnxruntime.dll"))
                .map_err(|e| format!("load onnxruntime.dll from {}: {e}", dir.display()))?
                .commit();
            Ok(())
        })
        .clone()
        .map_err(|e| anyhow!(e))
}

/// The directory of the executable when the DLLs are next to it, else the
/// vendored `onnxruntime/bin` from build time.
fn runtime_dir() -> PathBuf {
    crate::paths::executable_dir()
        .ok()
        .filter(|dir| dir.join("onnxruntime.dll").is_file())
        .unwrap_or_else(|| PathBuf::from(env!("FILE_BUDDY_ONNXRUNTIME_BIN")))
}

/// Scale to unit length. A zero vector stays zero.
pub fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        vector.iter_mut().for_each(|x| *x /= norm);
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normalized_vector_has_unit_length() {
        let mut vector = vec![3.0, 4.0];
        normalize(&mut vector);
        assert_eq!(vector, vec![0.6, 0.8]);
        assert!((dot(&vector, &vector) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_zero_vector_stays_zero() {
        let mut vector = vec![0.0; 4];
        normalize(&mut vector);
        assert_eq!(vector, vec![0.0; 4]);
    }

    #[test]
    fn reads_the_model_config() {
        let config: Config = serde_json::from_str(
            r#"{"model_cfg": {"embed_dim": 512, "vision_cfg": {"image_size": 256},
                "text_cfg": {"context_length": 77}},
                "preprocess_cfg": {"mean": [0.0, 0.0, 0.0], "std": [1.0, 1.0, 1.0]}}"#,
        )
        .unwrap();
        assert_eq!(config.model_cfg.vision_cfg.image_size, 256);
        assert_eq!(config.model_cfg.text_cfg.context_length, 77);
    }
}
