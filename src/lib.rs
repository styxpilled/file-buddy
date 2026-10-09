//! file-buddy: the data layer, and the window that shows it. Everything except
//! `ui` is free of GPUI, so the whole pipeline can be tested without a window.
//!
//! `docs/source-layout.md` says what each directory is for.

pub mod actions;
pub mod clip;
pub mod config;
pub mod db;
pub mod ffmpeg;
pub mod index;
pub mod media;
pub mod ocr;
pub mod paths;
pub mod search;
pub mod speech;
pub mod studio;
pub mod ui;
pub mod worker;

// Re-exported so a test or a front end needs no direct dependency on it, and
// so only one build of SQLite exists.
pub use rusqlite;
