//! The image encoder. This runs only in the `clip` worker process: `fb worker
//! clip`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ort::ep;
use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};

use crate::clip::{self, Config, Frame};
use crate::ffmpeg::frames;
use crate::paths;

/// One file to embed. The worker gives FFmpeg the path and reads pixels.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub path: PathBuf,
    /// None for a still, which gives its single image.
    pub duration: Option<f64>,
}

/// The worker process: load the model once, then embed the frames of each file
/// it gets.
pub fn serve() -> Result<()> {
    let mut encoder = ImageEncoder::load(&paths::clip_model()?)?;
    crate::worker::serve(|request: Request| {
        let pixels = frames::pixels(&request.path, request.duration, encoder.size)?;
        if pixels.is_empty() {
            bail!("FFmpeg gave no frames");
        }
        let images = pixels.iter().map(|p| p.rgb.as_slice()).collect::<Vec<_>>();
        let vectors = encoder.embed(&images)?;
        Ok(pixels
            .iter()
            .zip(vectors)
            .map(|(pixels, vector)| Frame {
                time_ms: pixels.time_ms,
                vector,
            })
            .collect::<Vec<_>>())
    })
}

pub struct ImageEncoder {
    session: Session,
    size: u32,
    mean: [f32; 3],
    std: [f32; 3],
}

impl ImageEncoder {
    /// On the GPU through DirectML. ONNX Runtime falls back to the CPU for any
    /// operator DirectML does not have.
    pub fn load(dir: &Path) -> Result<Self> {
        clip::runtime()?;
        let config = Config::read(dir)?;
        let path = dir.join("visual.onnx");
        let session = Session::builder()?
            .with_execution_providers([ep::DirectML::default().build()])
            .map_err(ort::Error::<()>::from)?
            .commit_from_file(&path)
            .with_context(|| format!("load {}", path.display()))?;
        Ok(Self {
            session,
            size: config.model_cfg.vision_cfg.image_size,
            mean: config.preprocess_cfg.mean,
            std: config.preprocess_cfg.std,
        })
    }

    /// Embed square `rgb24` images of the size the model takes, in one batch.
    pub fn embed(&mut self, images: &[&[u8]]) -> Result<Vec<Vec<f32>>> {
        let side = self.size as usize;
        let mut input = Vec::with_capacity(images.len() * 3 * side * side);
        for image in images {
            input.extend(to_planes(image, side, self.mean, self.std)?);
        }
        let shape = [images.len(), 3, side, side];
        let outputs = self
            .session
            .run(ort::inputs![Tensor::from_array((shape, input))?])?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>()?;
        let mut vectors = data
            .chunks_exact(data.len() / images.len())
            .map(<[f32]>::to_vec)
            .collect::<Vec<_>>();
        vectors.iter_mut().for_each(|v| clip::normalize(v));
        Ok(vectors)
    }
}

/// Interleaved `rgb24` rows to three planes of floats, scaled to 0..1 and
/// normalized by the mean and standard deviation of the model.
fn to_planes(rgb: &[u8], side: usize, mean: [f32; 3], std: [f32; 3]) -> Result<Vec<f32>> {
    let pixels = side * side;
    if rgb.len() != pixels * 3 {
        bail!("a frame has {} bytes, not {}", rgb.len(), pixels * 3);
    }
    let mut planes = vec![0.0; pixels * 3];
    for (i, pixel) in rgb.chunks_exact(3).enumerate() {
        for channel in 0..3 {
            planes[channel * pixels + i] =
                (f32::from(pixel[channel]) / 255.0 - mean[channel]) / std[channel];
        }
    }
    Ok(planes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_pixels_become_planes() {
        // Two pixels: pure red, then white.
        let rgb = [255, 0, 0, 255, 255, 255, 0, 0, 0, 0, 0, 0];
        let planes = to_planes(&rgb, 2, [0.0; 3], [1.0; 3]).unwrap();
        assert_eq!(&planes[0..4], &[1.0, 1.0, 0.0, 0.0]);
        assert_eq!(&planes[4..8], &[0.0, 1.0, 0.0, 0.0]);
        assert_eq!(&planes[8..12], &[0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn the_mean_and_deviation_apply_per_channel() {
        let planes = to_planes(&[255, 0, 255], 1, [0.5, 0.0, 0.0], [0.5, 1.0, 0.5]).unwrap();
        assert_eq!(planes, vec![1.0, 0.0, 2.0]);
    }

    #[test]
    fn a_frame_of_the_wrong_size_is_an_error() {
        assert!(to_planes(&[0; 5], 1, [0.0; 3], [1.0; 3]).is_err());
    }
}
