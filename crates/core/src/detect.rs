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

// ── FFmpeg-based probing ───────────────────────────────────────

#[cfg(feature = "ffmpeg")]
mod ffmpeg_probe {
    use super::*;
    use crate::error::ProbeError;

    /// Probe a media file and return detailed information.
    pub fn probe_file(path: &Path) -> std::result::Result<MediaInfo, ProbeError> {
        crate::init();

        let ctx = ffmpeg_next::format::input(&path).map_err(|e| {
            ProbeError::OpenFailed(format!("{}: {}", path.display(), e))
        })?;

        let format_name = ctx.format().name().to_string();
        let container = Container::from_path(path)
            .or_else(|| guess_container_from_format(&format_name));

        let duration = if ctx.duration() > 0 {
            Some(Duration::from_micros(ctx.duration() as u64))
        } else {
            None
        };

        let bitrate = if ctx.bit_rate() > 0 {
            Some(ctx.bit_rate() as u64)
        } else {
            None
        };

        let metadata = ctx
            .metadata()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let streams = ctx
            .streams()
            .enumerate()
            .map(|(idx, stream)| build_stream_info(idx, &stream))
            .collect();

        Ok(MediaInfo {
            container,
            format_name,
            duration,
            bitrate,
            streams,
            metadata,
        })
    }

    fn build_stream_info(
        index: usize,
        stream: &ffmpeg_next::format::stream::Stream,
    ) -> StreamInfo {
        let codec_params = stream.parameters();
        let codec_ctx =
            ffmpeg_next::codec::context::Context::from_parameters(codec_params.clone()).ok();

        let medium = codec_params.medium();
        let stream_type = match medium {
            ffmpeg_next::media::Type::Video => StreamType::Video,
            ffmpeg_next::media::Type::Audio => StreamType::Audio,
            ffmpeg_next::media::Type::Subtitle => StreamType::Subtitle,
            ffmpeg_next::media::Type::Data => StreamType::Data,
            ffmpeg_next::media::Type::Attachment => StreamType::Attachment,
            _ => StreamType::Unknown,
        };

        let codec_id = codec_params.id();
        let codec_name = codec_id
            .name()
            .to_string();

        let codec_long_name = ffmpeg_next::codec::decoder::find(codec_id)
            .map(|c| c.long_name().to_string());

        let bitrate = if stream.parameters().bit_rate() > 0 {
            Some(stream.parameters().bit_rate() as u64)
        } else {
            None
        };

        let time_base = stream.time_base();
        let duration = if stream.duration() > 0 {
            let secs = stream.duration() as f64 * time_base.0 as f64 / time_base.1 as f64;
            Some(Duration::from_secs_f64(secs))
        } else {
            None
        };

        let (width, height, fps, pixel_format) = if stream_type == StreamType::Video {
            let w = codec_ctx.as_ref().and_then(|c| {
                c.clone().decoder().ok().and_then(|d| d.video().ok().map(|v| v.width()))
            });
            let h = codec_ctx.as_ref().and_then(|c| {
                c.clone().decoder().ok().and_then(|d| d.video().ok().map(|v| v.height()))
            });
            let rate = stream.avg_frame_rate();
            let f = if rate.1 > 0 {
                Some(rate.0 as f64 / rate.1 as f64)
            } else {
                None
            };
            let pf = codec_ctx.as_ref().and_then(|c| {
                c.clone()
                    .decoder()
                    .ok()
                    .and_then(|d| d.video().ok().map(|v| format!("{:?}", v.format())))
            });
            (w, h, f, pf)
        } else {
            (None, None, None, None)
        };

        let (sample_rate, channels, sample_format) = if stream_type == StreamType::Audio {
            let sr = codec_ctx.as_ref().and_then(|c| {
                c.clone()
                    .decoder()
                    .ok()
                    .and_then(|d| d.audio().ok().map(|a| a.rate()))
            });
            let ch = codec_ctx.as_ref().and_then(|c| {
                c.clone()
                    .decoder()
                    .ok()
                    .and_then(|d| d.audio().ok().map(|a| a.channels() as u16))
            });
            let sf = codec_ctx.as_ref().and_then(|c| {
                c.clone()
                    .decoder()
                    .ok()
                    .and_then(|d| d.audio().ok().map(|a| format!("{:?}", a.format())))
            });
            (sr, ch, sf)
        } else {
            (None, None, None)
        };

        let metadata = stream
            .metadata()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        StreamInfo {
            index,
            stream_type,
            codec_name,
            codec_long_name,
            bitrate,
            duration,
            width,
            height,
            fps,
            pixel_format,
            sample_rate,
            channels,
            sample_format,
            metadata,
        }
    }

    fn guess_container_from_format(format_name: &str) -> Option<Container> {
        let first = format_name.split(',').next().unwrap_or(format_name);
        match first {
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
        }
    }
}

#[cfg(feature = "ffmpeg")]
pub use ffmpeg_probe::probe_file;

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
