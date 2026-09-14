//! What ffprobe says about a file.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::ffmpeg::{self, Tool};
use crate::media::Kind;

/// More than one frame at 25 fps. Anything longer is a moving picture, not a
/// still, whatever the extension says.
const ANIMATED_SECONDS: f64 = 0.04;

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub kind: Kind,
    pub duration: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

/// Run ffprobe over one file. An error here means FFmpeg cannot read the file.
pub fn probe(path: &Path, extension_kind: Kind) -> Result<Probe> {
    let mut command = ffmpeg::command(Tool::Ffprobe);
    command
        .args(["-print_format", "json", "-show_format", "-show_streams"])
        .arg(path);
    let stdout = ffmpeg::output(command).context("ffprobe")?;
    parse(&String::from_utf8_lossy(&stdout), extension_kind)
}

/// Read the ffprobe JSON. Separate from the process call so it can be tested.
pub fn parse(json: &str, extension_kind: Kind) -> Result<Probe> {
    let output: Output = serde_json::from_str(json).context("parse the ffprobe output")?;

    let video = output
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"));
    let audio = output
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"));
    if video.is_none() && audio.is_none() {
        bail!("no audio or video stream");
    }

    let duration = output
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .or_else(|| video.and_then(|s| s.duration.as_deref()))
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d > 0.0);

    let frames = video
        .and_then(|s| s.nb_frames.as_deref())
        .and_then(|f| f.parse::<i64>().ok());

    let kind = match extension_kind {
        Kind::Video => Kind::Video,
        _ => {
            let moving = frames.is_some_and(|f| f > 1)
                || (frames.is_none() && duration.is_some_and(|d| d > ANIMATED_SECONDS));
            if moving { Kind::Animated } else { Kind::Still }
        }
    };

    Ok(Probe {
        kind,
        // A still has no duration, whatever the container claims.
        duration: if kind == Kind::Still { None } else { duration },
        width: video.and_then(|s| s.width),
        height: video.and_then(|s| s.height),
        video_codec: video.and_then(|s| s.codec_name.clone()),
        audio_codec: audio.and_then(|s| s.codec_name.clone()),
    })
}

#[derive(Deserialize)]
struct Output {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    duration: Option<String>,
    nb_frames: Option<String>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: &str = r#"{
        "streams": [
            {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"nb_frames":"300"},
            {"codec_type":"audio","codec_name":"aac"}
        ],
        "format": {"duration":"12.500000"}
    }"#;

    #[test]
    fn reads_a_video() {
        let probe = parse(VIDEO, Kind::Video).unwrap();
        assert_eq!(probe.kind, Kind::Video);
        assert_eq!(probe.duration, Some(12.5));
        assert_eq!(probe.width, Some(1920));
        assert_eq!(probe.video_codec.as_deref(), Some("h264"));
        assert_eq!(probe.audio_codec.as_deref(), Some("aac"));
    }

    #[test]
    fn a_png_is_a_still() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"png","width":8,"height":8,"nb_frames":"1"}],"format":{"duration":"0.040000"}}"#;
        let probe = parse(json, Kind::Still).unwrap();
        assert_eq!(probe.kind, Kind::Still);
        assert_eq!(probe.duration, None);
    }

    #[test]
    fn an_animated_webp_keeps_its_still_extension_but_is_animated() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"webp","width":8,"height":8,"nb_frames":"30"}],"format":{"duration":"1.200000"}}"#;
        let probe = parse(json, Kind::Still).unwrap();
        assert_eq!(probe.kind, Kind::Animated);
        assert_eq!(probe.duration, Some(1.2));
    }

    #[test]
    fn a_gif_with_no_frame_count_is_animated_when_it_lasts() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"gif","width":8,"height":8}],"format":{"duration":"2.000000"}}"#;
        assert_eq!(parse(json, Kind::Animated).unwrap().kind, Kind::Animated);
    }

    #[test]
    fn a_missing_duration_is_none_not_zero() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"png","width":8,"height":8}],"format":{"duration":"N/A"}}"#;
        let probe = parse(json, Kind::Still).unwrap();
        assert_eq!(probe.duration, None);
    }

    #[test]
    fn a_file_with_no_stream_is_an_error() {
        assert!(parse(r#"{"streams":[],"format":{}}"#, Kind::Video).is_err());
    }
}
