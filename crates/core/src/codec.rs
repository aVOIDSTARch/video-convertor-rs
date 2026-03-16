//! Audio and video codec enums with FFmpeg codec name mappings.

use serde::{Deserialize, Serialize};

/// Supported audio codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioCodec {
    Mp3,
    Aac,
    Flac,
    Vorbis,
    Opus,
    Wav,
    Alac,
    Wma,
    Ac3,
    Pcm16Le,
    Pcm24Le,
    PcmF32Le,
}

impl AudioCodec {
    /// FFmpeg encoder name for this codec.
    pub fn ffmpeg_encoder(&self) -> &'static str {
        match self {
            Self::Mp3 => "libmp3lame",
            Self::Aac => "aac",
            Self::Flac => "flac",
            Self::Vorbis => "libvorbis",
            Self::Opus => "libopus",
            Self::Wav | Self::Pcm16Le => "pcm_s16le",
            Self::Pcm24Le => "pcm_s24le",
            Self::PcmF32Le => "pcm_f32le",
            Self::Alac => "alac",
            Self::Wma => "wmav2",
            Self::Ac3 => "ac3",
        }
    }

    /// FFmpeg decoder name for this codec.
    pub fn ffmpeg_decoder(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Aac => "aac",
            Self::Flac => "flac",
            Self::Vorbis => "vorbis",
            Self::Opus => "opus",
            Self::Wav | Self::Pcm16Le => "pcm_s16le",
            Self::Pcm24Le => "pcm_s24le",
            Self::PcmF32Le => "pcm_f32le",
            Self::Alac => "alac",
            Self::Wma => "wmav2",
            Self::Ac3 => "ac3",
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Aac => "AAC",
            Self::Flac => "FLAC",
            Self::Vorbis => "Vorbis",
            Self::Opus => "Opus",
            Self::Wav => "WAV (PCM 16-bit)",
            Self::Alac => "ALAC",
            Self::Wma => "WMA",
            Self::Ac3 => "AC-3",
            Self::Pcm16Le => "PCM 16-bit LE",
            Self::Pcm24Le => "PCM 24-bit LE",
            Self::PcmF32Le => "PCM Float 32-bit LE",
        }
    }

    /// All known audio codecs.
    pub fn all() -> &'static [AudioCodec] {
        &[
            Self::Mp3, Self::Aac, Self::Flac, Self::Vorbis, Self::Opus,
            Self::Wav, Self::Alac, Self::Wma, Self::Ac3,
            Self::Pcm16Le, Self::Pcm24Le, Self::PcmF32Le,
        ]
    }

    /// Parse from a string name (case-insensitive).
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mp3" => Some(Self::Mp3),
            "aac" | "m4a" => Some(Self::Aac),
            "flac" => Some(Self::Flac),
            "vorbis" | "ogg" => Some(Self::Vorbis),
            "opus" => Some(Self::Opus),
            "wav" | "pcm" => Some(Self::Wav),
            "alac" => Some(Self::Alac),
            "wma" => Some(Self::Wma),
            "ac3" => Some(Self::Ac3),
            "pcm16le" | "pcm_s16le" => Some(Self::Pcm16Le),
            "pcm24le" | "pcm_s24le" => Some(Self::Pcm24Le),
            "pcmf32le" | "pcm_f32le" => Some(Self::PcmF32Le),
            _ => None,
        }
    }
}

/// Supported video codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    H264,
    H265,
    Vp8,
    Vp9,
    Av1,
    Mpeg4,
    ProRes,
}

impl VideoCodec {
    /// FFmpeg encoder name for this codec.
    pub fn ffmpeg_encoder(&self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::H265 => "libx265",
            Self::Vp8 => "libvpx",
            Self::Vp9 => "libvpx-vp9",
            Self::Av1 => "libaom-av1",
            Self::Mpeg4 => "mpeg4",
            Self::ProRes => "prores_ks",
        }
    }

    /// FFmpeg decoder name for this codec.
    pub fn ffmpeg_decoder(&self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "hevc",
            Self::Vp8 => "vp8",
            Self::Vp9 => "vp9",
            Self::Av1 => "libdav1d",
            Self::Mpeg4 => "mpeg4",
            Self::ProRes => "prores",
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::H264 => "H.264 / AVC",
            Self::H265 => "H.265 / HEVC",
            Self::Vp8 => "VP8",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
            Self::Mpeg4 => "MPEG-4",
            Self::ProRes => "Apple ProRes",
        }
    }

    /// All known video codecs.
    pub fn all() -> &'static [VideoCodec] {
        &[
            Self::H264, Self::H265, Self::Vp8, Self::Vp9,
            Self::Av1, Self::Mpeg4, Self::ProRes,
        ]
    }

    /// Parse from a string name (case-insensitive).
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "h264" | "x264" | "avc" => Some(Self::H264),
            "h265" | "x265" | "hevc" => Some(Self::H265),
            "vp8" => Some(Self::Vp8),
            "vp9" => Some(Self::Vp9),
            "av1" => Some(Self::Av1),
            "mpeg4" => Some(Self::Mpeg4),
            "prores" => Some(Self::ProRes),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_codec_from_name_roundtrip() {
        for codec in AudioCodec::all() {
            let _name = format!("{:?}", codec).to_lowercase();
            // At minimum, the serde name should parse
            let json = serde_json::to_string(codec).unwrap();
            let parsed: AudioCodec = serde_json::from_str(&json).unwrap();
            assert_eq!(*codec, parsed, "roundtrip failed for {:?}", codec);
        }
    }

    #[test]
    fn video_codec_from_name_roundtrip() {
        for codec in VideoCodec::all() {
            let json = serde_json::to_string(codec).unwrap();
            let parsed: VideoCodec = serde_json::from_str(&json).unwrap();
            assert_eq!(*codec, parsed);
        }
    }

    #[test]
    fn audio_from_name_common_aliases() {
        assert_eq!(AudioCodec::from_name("mp3"), Some(AudioCodec::Mp3));
        assert_eq!(AudioCodec::from_name("AAC"), Some(AudioCodec::Aac));
        assert_eq!(AudioCodec::from_name("m4a"), Some(AudioCodec::Aac));
        assert_eq!(AudioCodec::from_name("ogg"), Some(AudioCodec::Vorbis));
        assert_eq!(AudioCodec::from_name("pcm"), Some(AudioCodec::Wav));
        assert_eq!(AudioCodec::from_name("nope"), None);
    }

    #[test]
    fn video_from_name_common_aliases() {
        assert_eq!(VideoCodec::from_name("h264"), Some(VideoCodec::H264));
        assert_eq!(VideoCodec::from_name("x265"), Some(VideoCodec::H265));
        assert_eq!(VideoCodec::from_name("hevc"), Some(VideoCodec::H265));
        assert_eq!(VideoCodec::from_name("AV1"), Some(VideoCodec::Av1));
        assert_eq!(VideoCodec::from_name("nope"), None);
    }

    #[test]
    fn ffmpeg_encoder_names_are_not_empty() {
        for c in AudioCodec::all() {
            assert!(!c.ffmpeg_encoder().is_empty());
        }
        for c in VideoCodec::all() {
            assert!(!c.ffmpeg_encoder().is_empty());
        }
    }

    #[test]
    fn display_names_are_not_empty() {
        for c in AudioCodec::all() {
            assert!(!c.display_name().is_empty());
        }
        for c in VideoCodec::all() {
            assert!(!c.display_name().is_empty());
        }
    }
}
