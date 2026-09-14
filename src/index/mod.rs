//! Everything that fills the database from the files on disk: the walk over the
//! roots, the durable job queue and the workers that drain it.

pub mod jobs;
pub mod pipeline;
pub mod scan;
pub mod stages;

pub use pipeline::{PipelineStats, StageStats, run};
pub use scan::ScanStats;
