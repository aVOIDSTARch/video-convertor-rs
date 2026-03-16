//! Container format enum with extension/MIME mappings and codec compatibility.

use crate::codec::{AudioCodec, VideoCodec};
use serde::{Deserialize, Serialize};

/// Supported container/muxer formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Container {
    Mp4,
    Mkv,
    WebM,
    Avi,
    Mov,
    Flv,
    Ogg,
    Flac,
    Wav,
    Mp3,
    M4a,
    M4b,
    Ts,
    Gif,
}

impl Container {
    /// File extension (without dot).
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::WebM => "webm",
            Self::Avi => "avi",
            Self::Mov => "mov",
            Self::Flv => "flv",
            Self::Ogg => "ogg",
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::M4b => "m4b",
            Self::Ts => "ts",
            Self::Gif => "gif",
        }
    }

    /// MIME type.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Mp4 => "video/mp4",
            Self::Mkv => "video/x-matroska",
            Self::WebM => "video/webm",
            Self::Avi => "video/x-msvideo",
            Self::Mov => "video/quicktime",
            Self::Flv => "video/x-flv",
            Self::Ogg => "audio/ogg",
            Self::Flac => "audio/flac",
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::M4a => "audio/x-m4a",
            Self::M4b => "audio/x-m4b",
            Self::Ts => "video/mp2t",
            Self::Gif => "image/gif",
        }
    }

    /// FFmpeg muxer/format name.
    pub fn ffmpeg_format(&self) -> &'static str {
        match self {
            Self::Mp4 | Self::M4a | Self::M4b => "mp4",
            Self::Mkv => "matroska",
            Self::WebM => "webm",
            Self::Avi => "avi",
            Self::Mov => "mov",
            Self::Flv => "flv",
            Self::Ogg => "ogg",
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Ts => "mpegts",
            Self::Gif => "gif",
        }
    }

    /// Whether this container can hold video streams.
    pub fn supports_video(&self) -> bool {
        matches!(
            self,
            Self::Mp4 | Self::Mkv | Self::WebM | Self::Avi
                | Self::Mov | Self::Flv | Self::Ts | Self::Gif
        )
    }

    /// Whether this container can hold audio streams.
    pub fn supports_audio(&self) -> bool {
        !matches!(self, Self::Gif)
    }

    /// Whether the given audio codec is compatible with this container.
    pub fn supports_audio_codec(&self, codec: AudioCodec) -> bool {
        match self {
            Self::Mp4 | Self::M4a | Self::M4b | Self::Mov => matches!(
                codec,
                AudioCodec::Aac | AudioCodec::Alac | AudioCodec::Ac3
                    | AudioCodec::Flac | AudioCodec::Mp3 | AudioCodec::Opus
                    | AudioCodec::Pcm16Le | AudioCodec::Pcm24Le | AudioCodec::PcmF32Le
            ),
            Self::Mkv => true, // MKV accepts nearly everything
            Self::WebM => matches!(codec, AudioCodec::Vorbis | AudioCodec::Opus),
            Self::Avi => matches!(
                codec,
                AudioCodec::Mp3 | AudioCodec::Ac3 | AudioCodec::Pcm16Le
                    | AudioCodec::Pcm24Le | AudioCodec::PcmF32Le
            ),
            Self::Flv => matches!(codec, AudioCodec::Aac | AudioCodec::Mp3),
            Self::Ogg => matches!(codec, AudioCodec::Vorbis | AudioCodec::Opus | AudioCodec::Flac),
            Self::Flac => matches!(codec, AudioCodec::Flac),
            Self::Wav => matches!(
                codec,
                AudioCodec::Wav | AudioCodec::Pcm16Le | AudioCodec::Pcm24Le | AudioCodec::PcmF32Le
            ),
            Self::Mp3 => matches!(codec, AudioCodec::Mp3),
            Self::Ts => matches!(codec, AudioCodec::Aac | AudioCodec::Mp3 | AudioCodec::Ac3),
            Self::Gif => false,
        }
    }

    /// Whether the given video codec is compatible with this container.
    pub fn supports_video_codec(&self, codec: VideoCodec) -> bool {
        match self {
            Self::Mp4 | Self::Mov => matches!(
                codec,
                VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Av1
                    | VideoCodec::Mpeg4 | VideoCodec::ProRes | VideoCodec::Vp9
            ),
            Self::Mkv => true,
            Self::WebM => matches!(codec, VideoCodec::Vp8 | VideoCodec::Vp9 | VideoCodec::Av1),
            Self::Avi => matches!(
                codec,
                VideoCodec::H264 | VideoCodec::Mpeg4
            ),
            Self::Flv => matches!(codec, VideoCodec::H264),
            Self::Ts => matches!(codec, VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Mpeg4),
            Self::Gif => false, // GIF uses its own codec internally
            Self::Ogg | Self::Flac | Self::Wav | Self::Mp3 | Self::M4a | Self::M4b => false,
        }
    }

    /// Whether this container requires seekable output (for the moov atom, etc.).
    pub fn requires_seekable_output(&self) -> bool {
        matches!(self, Self::Mp4 | Self::M4a | Self::M4b | Self::Mov)
    }

    /// All known containers.
    pub fn all() -> &'static [Container] {
        &[
            Self::Mp4, Self::Mkv, Self::WebM, Self::Avi, Self::Mov,
            Self::Flv, Self::Ogg, Self::Flac, Self::Wav, Self::Mp3,
            Self::M4a, Self::M4b, Self::Ts, Self::Gif,
        ]
    }

    /// Parse from file extension or name (case-insensitive).
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().trim_start_matches('.').as_ref() {
            "mp4" => Some(Self::Mp4),
            "mkv" | "matroska" => Some(Self::Mkv),
            "webm" => Some(Self::WebM),
            "avi" => Some(Self::Avi),
            "mov" => Some(Self::Mov),
            "flv" => Some(Self::Flv),
            "ogg" | "oga" => Some(Self::Ogg),
            "flac" => Some(Self::Flac),
            "wav" => Some(Self::Wav),
            "mp3" => Some(Self::Mp3),
            "m4a" => Some(Self::M4a),
            "m4b" => Some(Self::M4b),
            "ts" | "mts" | "m2ts" => Some(Self::Ts),
            "gif" => Some(Self::Gif),
            _ => None,
        }
    }

    /// Infer container from a file path's extension.
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_roundtrip() {
        for c in Container::all() {
            assert_eq!(Container::from_name(c.extension()), Some(*c));
        }
    }

    #[test]
    fn from_name_with_dot_prefix() {
        assert_eq!(Container::from_name(".mp4"), Some(Container::Mp4));
        assert_eq!(Container::from_name(".MKV"), Some(Container::Mkv));
    }

    #[test]
    fn from_path_works() {
        use std::path::Path;
        assert_eq!(
            Container::from_path(Path::new("/foo/bar.mp4")),
            Some(Container::Mp4)
        );
        assert_eq!(Container::from_path(Path::new("no_ext")), None);
    }

    #[test]
    fn audio_only_containers_dont_support_video() {
        assert!(!Container::Ogg.supports_video());
        assert!(!Container::Flac.supports_video());
        assert!(!Container::Wav.supports_video());
        assert!(!Container::Mp3.supports_video());
        assert!(!Container::M4a.supports_video());
    }

    #[test]
    fn gif_has_no_audio() {
        assert!(!Container::Gif.supports_audio());
    }

    #[test]
    fn webm_only_allows_webm_codecs() {
        assert!(Container::WebM.supports_video_codec(VideoCodec::Vp9));
        assert!(Container::WebM.supports_video_codec(VideoCodec::Av1));
        assert!(!Container::WebM.supports_video_codec(VideoCodec::H264));
        assert!(Container::WebM.supports_audio_codec(AudioCodec::Opus));
        assert!(!Container::WebM.supports_audio_codec(AudioCodec::Mp3));
    }

    #[test]
    fn mkv_supports_everything() {
        for vc in VideoCodec::all() {
            assert!(Container::Mkv.supports_video_codec(*vc));
        }
    }

    #[test]
    fn seekable_output_containers() {
        assert!(Container::Mp4.requires_seekable_output());
        assert!(Container::Mov.requires_seekable_output());
        assert!(!Container::Mkv.requires_seekable_output());
        assert!(!Container::Ts.requires_seekable_output());
    }

    #[test]
    fn mime_types_are_valid() {
        for c in Container::all() {
            assert!(c.mime_type().contains('/'));
        }
    }

    #[test]
    fn ffmpeg_format_names_not_empty() {
        for c in Container::all() {
            assert!(!c.ffmpeg_format().is_empty());
        }
    }

    #[test]
    fn serde_roundtrip() {
        for c in Container::all() {
            let json = serde_json::to_string(c).unwrap();
            let parsed: Container = serde_json::from_str(&json).unwrap();
            assert_eq!(*c, parsed);
        }
    }
}
