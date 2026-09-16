use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to cast FFmpeg element")]
    Cast,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("invalid URI")]
    Uri,
    #[error("failed to get media capabilities")]
    Caps,
    #[error("failed to sync with playback")]
    Sync,
    #[error("invalid framerate: {0}")]
    Framerate(f64),
}
