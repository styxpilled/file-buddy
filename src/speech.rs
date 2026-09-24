//! Speech to text with `whisper-rs`, on the GPU through Vulkan. This runs only in
//! the `whisper` worker process: `fb worker whisper`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    WhisperTokenId,
};

use crate::ffmpeg::audio;
use crate::paths;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// The mean log probability of the text tokens. NULL for a segment from the
    /// FFmpeg filter, which gave no probabilities.
    pub confidence: Option<f32>,
    pub no_speech_prob: Option<f32>,
}

/// One file to transcribe. The worker gives FFmpeg the path and reads samples.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub path: PathBuf,
}

/// The worker process: load the model once, then transcribe each file it gets.
pub fn serve() -> Result<()> {
    whisper_rs::install_logging_hooks();
    let mut model = Model::load(&paths::whisper_model()?)?;
    crate::worker::serve(|request: Request| model.transcribe(&audio::samples(&request.path)?))
}

pub struct Model {
    state: WhisperState,
    /// Token ids from this one up are special and timestamp tokens, not text.
    eot: WhisperTokenId,
}

impl Model {
    pub fn load(path: &Path) -> Result<Self> {
        let params = WhisperContextParameters {
            flash_attn: true,
            ..Default::default()
        };
        let context = WhisperContext::new_with_params(path, params)
            .with_context(|| format!("load {}", path.display()))?;
        Ok(Self {
            eot: context.token_eot(),
            state: context.create_state().context("create the whisper state")?,
        })
    }

    /// Transcribe 16 kHz mono samples. The parameters are the ones the FFmpeg
    /// filter used, over the whole file instead of 30 s windows.
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<Vec<Segment>> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 5 });
        params.set_language(Some("auto"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        self.state.full(params, samples).context("whisper")?;

        let mut segments = Vec::new();
        for segment in self.state.as_iter() {
            let mut sum = 0.0;
            let mut count = 0;
            for i in 0..segment.n_tokens() {
                if let Some(token) = segment.get_token(i)
                    && token.token_id() < self.eot
                {
                    sum += token.token_data().plog;
                    count += 1;
                }
            }
            segments.push(Segment {
                start_ms: segment.start_timestamp() * 10,
                end_ms: segment.end_timestamp() * 10,
                text: segment.to_str_lossy()?.into_owned(),
                confidence: (count > 0).then(|| sum / count as f32),
                no_speech_prob: Some(segment.no_speech_probability()),
            });
        }
        Ok(segments)
    }
}
