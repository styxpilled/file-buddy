//! The text encoder. It runs in the process that searches, on the CPU: it is
//! small, and a query needs it on every keystroke.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::clip::{self, Config};

/// The token the model reads as padding after the end of the text.
const PAD: i64 = 0;

pub struct TextEncoder {
    session: Session,
    tokenizer: Tokenizer,
    context_length: usize,
}

impl TextEncoder {
    pub fn load(dir: &Path) -> Result<Self> {
        clip::runtime()?;
        let config = Config::read(dir)?;
        let path = dir.join("text.onnx");
        let session = Session::builder()?
            .commit_from_file(&path)
            .with_context(|| format!("load {}", path.display()))?;
        let path = dir.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&path)
            .map_err(|e| anyhow!(e))
            .with_context(|| format!("load {}", path.display()))?;
        Ok(Self {
            session,
            tokenizer,
            context_length: config.model_cfg.text_cfg.context_length,
        })
    }

    /// The normalized vector of one query.
    pub fn embed(&mut self, query: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(query, true)
            .map_err(|e| anyhow!(e))
            .context("tokenize")?;
        let ids = fit(encoding.get_ids(), self.context_length);
        let outputs = self
            .session
            .run(ort::inputs![Tensor::from_array(([1, ids.len()], ids))?])?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>()?;
        let mut vector = data.to_vec();
        clip::normalize(&mut vector);
        Ok(vector)
    }
}

/// The ids at the length the model takes: cut, keeping the end token last, or
/// padded.
fn fit(ids: &[u32], length: usize) -> Vec<i64> {
    let mut fitted: Vec<i64> = ids.iter().map(|&id| i64::from(id)).collect();
    if fitted.len() > length {
        let end = *fitted.last().unwrap_or(&PAD);
        fitted.truncate(length);
        fitted[length - 1] = end;
    }
    fitted.resize(length, PAD);
    fitted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_query_is_padded() {
        assert_eq!(fit(&[49406, 320, 49407], 5), vec![49406, 320, 49407, 0, 0]);
    }

    #[test]
    fn a_long_query_is_cut_and_keeps_its_end_token() {
        assert_eq!(
            fit(&[49406, 1, 2, 3, 4, 49407], 4),
            vec![49406, 1, 2, 49407]
        );
    }
}
