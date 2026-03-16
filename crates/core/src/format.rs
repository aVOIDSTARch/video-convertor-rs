//! Media format types: settings for audio/video encoding and overall output format.

use crate::codec::{AudioCodec, VideoCodec};
use crate::container::Container;
use serde::{Deserialize, Serialize};

/// Audio encoding settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub codec: AudioCodec,
    /// Bitrate in bits per second (e.g. 128_000 for 128kbps). None = codec default / VBR.
    pub bitrate: Option<u32>,
    /// Sample rate in Hz (e.g. 44100, 48000). None = same as source.
    pub sample_rate: Option<u32>,
    /// Number of channels. None = same as source.
    pub channels: Option<u16>,
}

impl AudioSettings {
    pub fn new(codec: AudioCodec) -> Self {
        Self {
            codec,
            bitrate: None,
            sample_rate: None,
            channels: None,
        }
    }

    pub fn with_bitrate(mut self, bps: u32) -> Self {
        self.bitrate = Some(bps);
        self
    }

    pub fn with_sample_rate(mut self, hz: u32) -> Self {
        self.sample_rate = Some(hz);
        self
    }

    pub fn with_channels(mut self, ch: u16) -> Self {
        self.channels = Some(ch);
        self
    }
}

/// Video encoding settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSettings {
    pub codec: VideoCodec,
    /// Constant Rate Factor (lower = better quality, bigger file). None = codec default.
    pub crf: Option<u8>,
    /// Target width in pixels. None = same as source.
    pub width: Option<u32>,
    /// Target height in pixels. None = same as source.
    pub height: Option<u32>,
    /// Frame rate. None = same as source.
    pub fps: Option<f64>,
    /// Encoder preset string (e.g. "medium", "slow"). None = encoder default.
    pub encoder_preset: Option<String>,
    /// Pixel format (e.g. "yuv420p"). None = encoder default.
    pub pixel_format: Option<String>,
}

impl VideoSettings {
    pub fn new(codec: VideoCodec) -> Self {
        Self {
            codec,
            crf: None,
            width: None,
            height: None,
            fps: None,
            encoder_preset: None,
            pixel_format: None,
        }
    }

    pub fn with_crf(mut self, crf: u8) -> Self {
        self.crf = Some(crf);
        self
    }

    pub fn with_resolution(mut self, w: u32, h: u32) -> Self {
        self.width = Some(w);
        self.height = Some(h);
        self
    }

    pub fn with_fps(mut self, fps: f64) -> Self {
        self.fps = Some(fps);
        self
    }

    pub fn with_encoder_preset(mut self, preset: impl Into<String>) -> Self {
        self.encoder_preset = Some(preset.into());
        self
    }

    pub fn with_pixel_format(mut self, fmt: impl Into<String>) -> Self {
        self.pixel_format = Some(fmt.into());
        self
    }
}

/// Complete output format specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFormat {
    pub container: Container,
    pub audio: Option<AudioSettings>,
    pub video: Option<VideoSettings>,
    /// If true, copy the audio stream without re-encoding.
    pub copy_audio: bool,
    /// If true, copy the video stream without re-encoding.
    pub copy_video: bool,
    /// Strip all audio streams from output.
    pub no_audio: bool,
    /// Strip all video streams from output.
    pub no_video: bool,
}

impl MediaFormat {
    /// Audio-only format.
    pub fn audio_only(container: Container, audio: AudioSettings) -> Self {
        Self {
            container,
            audio: Some(audio),
            video: None,
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: true,
        }
    }

    /// Video format with both streams.
    pub fn video(container: Container, video: VideoSettings, audio: AudioSettings) -> Self {
        Self {
            container,
            audio: Some(audio),
            video: Some(video),
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: false,
        }
    }

    /// Video-only (no audio).
    pub fn video_only(container: Container, video: VideoSettings) -> Self {
        Self {
            container,
            audio: None,
            video: Some(video),
            copy_audio: false,
            copy_video: false,
            no_audio: true,
            no_video: false,
        }
    }

    /// Copy all streams into a new container (remux).
    pub fn remux(container: Container) -> Self {
        Self {
            container,
            audio: None,
            video: None,
            copy_audio: true,
            copy_video: true,
            no_audio: false,
            no_video: false,
        }
    }

    /// Extract audio from a video file (copy codec).
    pub fn extract_audio(container: Container) -> Self {
        Self {
            container,
            audio: None,
            video: None,
            copy_audio: true,
            copy_video: false,
            no_audio: false,
            no_video: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_settings_builder() {
        let s = AudioSettings::new(AudioCodec::Mp3)
            .with_bitrate(128_000)
            .with_sample_rate(44100)
            .with_channels(1);
        assert_eq!(s.codec, AudioCodec::Mp3);
        assert_eq!(s.bitrate, Some(128_000));
        assert_eq!(s.sample_rate, Some(44100));
        assert_eq!(s.channels, Some(1));
    }

    #[test]
    fn video_settings_builder() {
        let s = VideoSettings::new(VideoCodec::H264)
            .with_crf(23)
            .with_resolution(1920, 1080)
            .with_fps(30.0)
            .with_encoder_preset("medium");
        assert_eq!(s.codec, VideoCodec::H264);
        assert_eq!(s.crf, Some(23));
        assert_eq!(s.width, Some(1920));
        assert_eq!(s.height, Some(1080));
        assert_eq!(s.fps, Some(30.0));
        assert_eq!(s.encoder_preset.as_deref(), Some("medium"));
    }

    #[test]
    fn audio_only_format() {
        let f = MediaFormat::audio_only(
            Container::Mp3,
            AudioSettings::new(AudioCodec::Mp3).with_bitrate(128_000),
        );
        assert_eq!(f.container, Container::Mp3);
        assert!(f.audio.is_some());
        assert!(f.video.is_none());
        assert!(f.no_video);
        assert!(!f.no_audio);
    }

    #[test]
    fn video_format() {
        let f = MediaFormat::video(
            Container::Mp4,
            VideoSettings::new(VideoCodec::H264).with_crf(23),
            AudioSettings::new(AudioCodec::Aac).with_bitrate(128_000),
        );
        assert_eq!(f.container, Container::Mp4);
        assert!(f.audio.is_some());
        assert!(f.video.is_some());
        assert!(!f.no_video);
        assert!(!f.no_audio);
    }

    #[test]
    fn remux_copies_both_streams() {
        let f = MediaFormat::remux(Container::Mkv);
        assert!(f.copy_audio);
        assert!(f.copy_video);
        assert!(f.audio.is_none());
        assert!(f.video.is_none());
    }

    #[test]
    fn extract_audio_strips_video() {
        let f = MediaFormat::extract_audio(Container::M4a);
        assert!(f.copy_audio);
        assert!(!f.copy_video);
        assert!(f.no_video);
    }

    #[test]
    fn serde_roundtrip() {
        let f = MediaFormat::video(
            Container::Mp4,
            VideoSettings::new(VideoCodec::H264),
            AudioSettings::new(AudioCodec::Aac),
        );
        let json = serde_json::to_string(&f).unwrap();
        let parsed: MediaFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.container, f.container);
    }
}
