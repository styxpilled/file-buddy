//! Speech to text with the FFmpeg `whisper` filter. The filter gives start, end
//! and text for each segment, and no probabilities.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::ffmpeg::{self, Tool};

/// The length of one transcription window, in seconds. The filter does not go
/// above 30, and shorter windows cut sentences and hallucinate more.
pub const WINDOW_SECONDS: u32 = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Transcribe the first audio track of one file.
pub fn transcribe(source: &Path, model: &Path) -> Result<Vec<Segment>> {
    let filter = format!(
        "aresample=16000,whisper=model={}:language=auto:queue={WINDOW_SECONDS}:\
         format=json:destination={}",
        filter_value(&model.to_string_lossy())?,
        filter_value("pipe:1")?,
    );
    let mut command = ffmpeg::command(Tool::Ffmpeg);
    command
        .arg("-i")
        .arg(source)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-af", &filter])
        .args(["-f", "null", "-"]);
    let stdout = ffmpeg::output(command).context("whisper")?;
    Ok(parse(&String::from_utf8_lossy(&stdout)))
}

/// One option value in a filter string. Quoted, with the colon escaped, which
/// is what a Windows path needs to pass both levels of filter parsing.
fn filter_value(value: &str) -> Result<String> {
    if value.contains('\'') {
        bail!("a filter value cannot hold a quote: {value}");
    }
    Ok(format!(
        "'{}'",
        value.replace('\\', "/").replace(':', "\\:")
    ))
}

/// Read the output, one JSON object per line. The filter does not escape quotes
/// in the text, so a line is read by position and not as JSON.
pub fn parse(output: &str) -> Vec<Segment> {
    output.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Segment> {
    let rest = line.trim().strip_prefix("{\"start\":")?;
    let (start, rest) = rest.split_once(",\"end\":")?;
    let (end, rest) = rest.split_once(",\"text\":\"")?;
    let text = rest.strip_suffix("\"}")?;
    Some(Segment {
        start_ms: start.trim().parse().ok()?,
        end_ms: end.trim().parse().ok()?,
        text: text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_line() {
        let output = "{\"start\":0,\"end\":3500,\"text\":\"I understand Freud.\"}\n\
                      {\"start\":3720,\"end\":5320,\"text\":\"But in my world.\"}\n";
        assert_eq!(
            parse(output),
            vec![
                Segment {
                    start_ms: 0,
                    end_ms: 3500,
                    text: "I understand Freud.".into()
                },
                Segment {
                    start_ms: 3720,
                    end_ms: 5320,
                    text: "But in my world.".into()
                },
            ]
        );
    }

    #[test]
    fn keeps_a_quote_the_filter_did_not_escape() {
        let line = "{\"start\":6500,\"end\":9980,\"text\":\"\"I'm going to go.\"\"}";
        assert_eq!(parse(line)[0].text, "\"I'm going to go.\"");
    }

    #[test]
    fn skips_a_line_that_is_not_a_segment() {
        assert!(parse("\n[whisper] something\n{\"start\":x}").is_empty());
    }

    #[test]
    fn escapes_a_windows_path() {
        assert_eq!(
            filter_value(r"C:\clips\models\a.bin").unwrap(),
            r"'C\:/clips/models/a.bin'"
        );
        assert!(filter_value("it's.bin").is_err());
    }
}
