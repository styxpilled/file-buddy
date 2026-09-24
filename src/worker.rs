//! Worker processes. A model runs in a child `fb` process, so a GPU driver fault
//! or a crash in native code cannot close the window. The parent writes one JSON
//! request per line to the child's standard input, and the child answers each
//! with one JSON line on its standard output.
//!
//! This file knows nothing about any model. `speech` is the whisper side.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A child process that starts on the first call, and starts again on the call
/// after it stops.
pub struct Worker {
    args: Vec<OsString>,
    running: Option<Running>,
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Worker {
    /// `fb` with these arguments.
    pub fn new(args: &[&str]) -> Self {
        Self {
            args: args.iter().map(OsString::from).collect(),
            running: None,
        }
    }

    /// Send one request and wait for its answer. If the process stops, the error
    /// says so, and the next call starts a new one.
    pub fn call<T: DeserializeOwned>(&mut self, request: &impl Serialize) -> Result<T> {
        let running = match &mut self.running {
            Some(running) => running,
            None => self.running.insert(Running::start(&self.args)?),
        };
        match running.exchange(request) {
            Ok(reply) => reply.map_err(|message| anyhow!(message)),
            Err(e) => {
                let status = self.running.take().map(Running::stop);
                Err(e.context(format!("the worker stopped ({})", describe(status))))
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.running.take().map(Running::stop);
    }
}

impl Running {
    fn start(args: &[OsString]) -> Result<Self> {
        let program = program();
        let mut child = Command::new(&program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("start {}", program.display()))?;
        Ok(Self {
            stdin: child.stdin.take().context("no stdin")?,
            stdout: BufReader::new(child.stdout.take().context("no stdout")?),
            child,
        })
    }

    fn exchange<T: DeserializeOwned>(
        &mut self,
        request: &impl Serialize,
    ) -> Result<Result<T, String>> {
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()?;

        let mut reply = String::new();
        if self.stdout.read_line(&mut reply)? == 0 {
            anyhow::bail!("no answer");
        }
        Ok(serde_json::from_str(&reply)?)
    }

    /// Nothing is in progress when this runs, so a kill loses no work.
    fn stop(mut self) -> std::io::Result<std::process::ExitStatus> {
        let _ = self.child.kill();
        self.child.wait()
    }
}

fn describe(status: Option<std::io::Result<std::process::ExitStatus>>) -> String {
    match status {
        Some(Ok(status)) => status.to_string(),
        Some(Err(e)) => e.to_string(),
        None => "not started".into(),
    }
}

/// The child side. Answer each request on standard input until it closes, which
/// is when the parent is done or gone. An error answers one request and does not
/// stop the process.
pub fn serve<R, T>(mut handle: impl FnMut(R) -> Result<T>) -> Result<()>
where
    R: DeserializeOwned,
    T: Serialize,
{
    let mut stdout = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let reply = serde_json::from_str(&line?)
            .map_err(anyhow::Error::from)
            .and_then(&mut handle)
            .map_err(|e| format!("{e:#}"));
        serde_json::to_writer(&mut stdout, &reply)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

/// Where `fb.exe` is. `FILE_BUDDY_FB` points at it. Without it, it is the one
/// next to the executable.
fn program() -> PathBuf {
    if let Some(path) = std::env::var_os("FILE_BUDDY_FB") {
        return PathBuf::from(path);
    }
    crate::paths::executable_dir()
        .map(|dir| dir.join("fb.exe"))
        .unwrap_or_else(|_| PathBuf::from("fb.exe"))
}
