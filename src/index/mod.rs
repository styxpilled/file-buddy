//! Everything that fills the database from the files on disk: the walk over the
//! roots, the durable job queue and the workers that drain it.

pub mod dropped;
pub mod jobs;
pub mod pipeline;
pub mod scan;
pub mod stages;
pub mod watch;

pub use pipeline::{embed_frames, read_text, run, transcribe};
