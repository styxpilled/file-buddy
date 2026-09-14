//! The data layer of file-buddy: the database, the scan and the stage pipeline.
//! Nothing here knows about the user interface.
//!
//! `docs/source-layout.md` says what each directory is for and where the work
//! of the later issues goes.

pub mod config;
pub mod db;
pub mod ffmpeg;
pub mod index;
pub mod media;
pub mod paths;
pub mod search;

// Re-exported so a test or a front end needs no direct dependency on it, and
// so only one build of SQLite exists.
pub use rusqlite;

pub use config::Config;
pub use db::Db;
pub use media::Kind;
