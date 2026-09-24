//! Every call out to the FFmpeg command line tools. The application never
//! links FFmpeg itself: a subprocess that crashes cannot stop the window.

pub mod audio;
pub mod frames;
pub mod probe;
pub mod thumbnail;

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
        }
    }
}

/// A command with standard input closed. FFmpeg reads standard input and
/// corrupts any loop that feeds it file names. `ffprobe` has no `-nostdin`
/// option and fails if it gets one, so closing the handle is the rule that
/// covers both tools.
pub fn command(tool: Tool) -> Command {
    let mut command = Command::new(path_of(tool));
    command.stdin(Stdio::null()).args(["-v", "error"]);
    if tool == Tool::Ffmpeg {
        command.args(["-nostdin", "-y"]);
    }
    command
}

/// Run the command and return what it wrote to standard output. A non-zero exit
/// becomes an error that holds the standard error text, which is what the `jobs`
/// row and the Review screen show.
pub fn output(mut command: Command) -> Result<Vec<u8>> {
    let output = command.output().context("run FFmpeg")?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        let message = message.lines().next_back().unwrap_or("no output").trim();
        bail!("{message}");
    }
    Ok(output.stdout)
}

/// Where the executable is. `FILE_BUDDY_FFMPEG` points at the directory that
/// holds both tools. Without it, the search order is the directory of the
/// executable, then the vendored `ffmpeg/bin` from build time, then `PATH`.
pub fn path_of(tool: Tool) -> PathBuf {
    let file = format!("{}.exe", tool.name());
    let candidates = [
        std::env::var_os("FILE_BUDDY_FFMPEG").map(PathBuf::from),
        crate::paths::executable_dir().ok(),
        Some(PathBuf::from(env!("FILE_BUDDY_FFMPEG_BIN"))),
    ];
    for dir in candidates.into_iter().flatten() {
        let candidate = dir.join(&file);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(tool.name())
}
