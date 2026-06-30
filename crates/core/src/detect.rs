//! Media file probing: extract stream info, codec, duration, metadata.

use crate::container::Container;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Complete information about a media file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    /// Detected container format.
    pub container: Option<Container>,
    /// Raw format name from FFmpeg (e.g. "matroska,webm").
    pub format_name: String,
    /// Total duration.
    pub duration: Option<Duration>,
    /// Total bitrate in bits/second.
    pub bitrate: Option<u64>,
    /// Streams found in the file.
    pub streams: Vec<StreamInfo>,
    /// File-level metadata (title, artist, etc.).
    pub metadata: HashMap<String, String>,
}

impl MediaInfo {
    /// First video stream, if any.
    pub fn video_stream(&self) -> Option<&StreamInfo> {
        self.streams.iter().find(|s| s.stream_type == StreamType::Video)
    }

    /// First audio stream, if any.
    pub fn audio_stream(&self) -> Option<&StreamInfo> {
        self.streams.iter().find(|s| s.stream_type == StreamType::Audio)
    }

    /// All video streams.
    pub fn video_streams(&self) -> Vec<&StreamInfo> {
        self.streams.iter().filter(|s| s.stream_type == StreamType::Video).collect()
    }

    /// All audio streams.
    pub fn audio_streams(&self) -> Vec<&StreamInfo> {
        self.streams.iter().filter(|s| s.stream_type == StreamType::Audio).collect()
    }

    /// Whether this file contains any video streams.
    pub fn has_video(&self) -> bool {
        self.streams.iter().any(|s| s.stream_type == StreamType::Video)
    }

    /// Whether this file contains any audio streams.
    pub fn has_audio(&self) -> bool {
        self.streams.iter().any(|s| s.stream_type == StreamType::Audio)
    }
}

/// Information about a single stream within a media file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    /// Stream index.
    pub index: usize,
    /// Stream type.
    pub stream_type: StreamType,
    /// Codec name (FFmpeg short name, e.g. "h264", "aac").
    pub codec_name: String,
    /// Human-readable codec description.
    pub codec_long_name: Option<String>,
    /// Bitrate in bits/second.
    pub bitrate: Option<u64>,
    /// Duration of this stream.
    pub duration: Option<Duration>,
    /// Video: width in pixels.
    pub width: Option<u32>,
    /// Video: height in pixels.
    pub height: Option<u32>,
    /// Video: frame rate as rational (num/den).
    pub fps: Option<f64>,
    /// Video: pixel format name.
    pub pixel_format: Option<String>,
    /// Audio: sample rate in Hz.
    pub sample_rate: Option<u32>,
    /// Audio: number of channels.
    pub channels: Option<u16>,
    /// Audio: sample format name.
    pub sample_format: Option<String>,
    /// Stream-level metadata.
    pub metadata: HashMap<String, String>,
}

/// Type of media stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamType {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
    Unknown,
}

/// Best-effort mapping from an ffprobe `format_name` (e.g. `"matroska,webm"`) to a
/// known [`Container`]. Used by the ffprobe-backed probe in [`crate::ffmpeg::probe`].
pub fn guess_container_from_format(format_name: &str) -> Option<Container> {
    for part in format_name.split(',') {
        let hit = match part.trim() {
            "matroska" | "webm" => Some(Container::Mkv),
            "mov" | "mp4" | "m4a" | "3gp" => Some(Container::Mp4),
            "avi" => Some(Container::Avi),
            "flv" => Some(Container::Flv),
            "ogg" => Some(Container::Ogg),
            "flac" => Some(Container::Flac),
            "wav" => Some(Container::Wav),
            "mp3" => Some(Container::Mp3),
            "mpegts" => Some(Container::Ts),
            "gif" => Some(Container::Gif),
            _ => None,
        };
        if hit.is_some() {
            return hit;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_type_serde_roundtrip() {
        for st in &[
            StreamType::Video,
            StreamType::Audio,
            StreamType::Subtitle,
            StreamType::Data,
        ] {
            let json = serde_json::to_string(st).unwrap();
            let parsed: StreamType = serde_json::from_str(&json).unwrap();
            assert_eq!(*st, parsed);
        }
    }

    #[test]
    fn media_info_helpers() {
        let info = MediaInfo {
            container: Some(Container::Mp4),
            format_name: "mov,mp4,m4a".to_string(),
            duration: Some(Duration::from_secs(60)),
            bitrate: Some(1_000_000),
            streams: vec![
                StreamInfo {
                    index: 0,
                    stream_type: StreamType::Video,
                    codec_name: "h264".to_string(),
                    codec_long_name: None,
                    bitrate: Some(800_000),
                    duration: None,
                    width: Some(1920),
                    height: Some(1080),
                    fps: Some(30.0),
                    pixel_format: Some("yuv420p".to_string()),
                    sample_rate: None,
                    channels: None,
                    sample_format: None,
                    metadata: HashMap::new(),
                },
                StreamInfo {
                    index: 1,
                    stream_type: StreamType::Audio,
                    codec_name: "aac".to_string(),
                    codec_long_name: None,
                    bitrate: Some(128_000),
                    duration: None,
                    width: None,
                    height: None,
                    fps: None,
                    pixel_format: None,
                    sample_rate: Some(44100),
                    channels: Some(2),
                    sample_format: Some("fltp".to_string()),
                    metadata: HashMap::new(),
                },
            ],
            metadata: HashMap::new(),
        };

        assert!(info.has_video());
        assert!(info.has_audio());
        assert_eq!(info.video_streams().len(), 1);
        assert_eq!(info.audio_streams().len(), 1);
        assert_eq!(info.video_stream().unwrap().width, Some(1920));
        assert_eq!(info.audio_stream().unwrap().sample_rate, Some(44100));
    }

    #[test]
    fn media_info_serde() {
        let info = MediaInfo {
            container: Some(Container::Mkv),
            format_name: "matroska".to_string(),
            duration: None,
            bitrate: None,
            streams: vec![],
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: MediaInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.format_name, "matroska");
    }
}
