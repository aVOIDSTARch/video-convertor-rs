//! Probe media files via `ffprobe -print_format json` into structured [`MediaInfo`].

use crate::container::Container;
use crate::detect::{guess_container_from_format, MediaInfo, StreamInfo, StreamType};
use crate::error::{MediaError, ProbeError};
use crate::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Probe `path` using `ffprobe`, returning structured stream/format information.
pub fn probe(ffprobe: &Path, path: &Path) -> Result<MediaInfo> {
    let output = Command::new(ffprobe)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|e| MediaError::ToolNotFound(format!("failed to run ffprobe: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProbeError::OpenFailed(format!(
            "{}: {}",
            path.display(),
            stderr.trim()
        ))
        .into());
    }

    let json: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| ProbeError::Ffmpeg(format!("invalid ffprobe JSON: {e}")))?;

    parse_media_info(&json, path)
}

fn parse_media_info(json: &Value, path: &Path) -> Result<MediaInfo> {
    let format = json.get("format").cloned().unwrap_or(Value::Null);

    let format_name = format
        .get("format_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let container = Container::from_path(path).or_else(|| guess_container_from_format(&format_name));

    let duration = format
        .get("duration")
        .and_then(parse_f64)
        .map(Duration::from_secs_f64);

    let bitrate = format.get("bit_rate").and_then(parse_u64);

    let metadata = parse_tags(format.get("tags"));

    let streams = json
        .get("streams")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_stream).collect())
        .unwrap_or_default();

    Ok(MediaInfo {
        container,
        format_name,
        duration,
        bitrate,
        streams,
        metadata,
    })
}

fn parse_stream(s: &Value) -> StreamInfo {
    let stream_type = match s.get("codec_type").and_then(Value::as_str) {
        Some("video") => StreamType::Video,
        Some("audio") => StreamType::Audio,
        Some("subtitle") => StreamType::Subtitle,
        Some("data") => StreamType::Data,
        Some("attachment") => StreamType::Attachment,
        _ => StreamType::Unknown,
    };

    let fps = s
        .get("avg_frame_rate")
        .and_then(Value::as_str)
        .or_else(|| s.get("r_frame_rate").and_then(Value::as_str))
        .and_then(parse_rational);

    StreamInfo {
        index: s.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
        stream_type,
        codec_name: s
            .get("codec_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        codec_long_name: s
            .get("codec_long_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        bitrate: s.get("bit_rate").and_then(parse_u64),
        duration: s.get("duration").and_then(parse_f64).map(Duration::from_secs_f64),
        width: s.get("width").and_then(Value::as_u64).map(|v| v as u32),
        height: s.get("height").and_then(Value::as_u64).map(|v| v as u32),
        fps,
        pixel_format: s.get("pix_fmt").and_then(Value::as_str).map(str::to_string),
        sample_rate: s.get("sample_rate").and_then(parse_u64).map(|v| v as u32),
        channels: s.get("channels").and_then(Value::as_u64).map(|v| v as u16),
        sample_format: s.get("sample_fmt").and_then(Value::as_str).map(str::to_string),
        metadata: parse_tags(s.get("tags")),
    }
}

fn parse_tags(tags: Option<&Value>) -> HashMap<String, String> {
    tags.and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// ffprobe emits numbers as JSON strings; accept both.
fn parse_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn parse_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Parse a rational like `"30000/1001"` into a float frame rate.
fn parse_rational(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/')?;
    let n: f64 = n.parse().ok()?;
    let d: f64 = d.parse().ok()?;
    if d == 0.0 {
        None
    } else {
        Some(n / d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rational_works() {
        assert!((parse_rational("30000/1001").unwrap() - 29.97).abs() < 0.01);
        assert!((parse_rational("25/1").unwrap() - 25.0).abs() < 0.001);
        assert!(parse_rational("0/0").is_none());
    }

    #[test]
    fn parse_media_info_from_json() {
        let json: Value = serde_json::from_str(
            r#"{
                "streams": [
                    {"index":0,"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"30/1","pix_fmt":"yuv420p","bit_rate":"800000"},
                    {"index":1,"codec_type":"audio","codec_name":"aac","sample_rate":"44100","channels":2,"sample_fmt":"fltp"}
                ],
                "format": {"format_name":"mov,mp4,m4a","duration":"60.0","bit_rate":"1000000","tags":{"title":"Test"}}
            }"#,
        )
        .unwrap();

        let info = parse_media_info(&json, Path::new("x.mp4")).unwrap();
        assert_eq!(info.streams.len(), 2);
        assert!(info.has_video());
        assert!(info.has_audio());
        assert_eq!(info.video_stream().unwrap().width, Some(1920));
        assert_eq!(info.audio_stream().unwrap().sample_rate, Some(44100));
        assert_eq!(info.duration, Some(Duration::from_secs(60)));
        assert_eq!(info.metadata.get("title").map(String::as_str), Some("Test"));
    }
}
