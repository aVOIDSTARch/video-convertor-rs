//! Built-in conversion presets for common use cases.

use crate::codec::{AudioCodec, VideoCodec};
use crate::container::Container;
use crate::format::{AudioSettings, MediaFormat, VideoSettings};
use serde::{Deserialize, Serialize};

/// A named, pre-configured conversion preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Machine-readable identifier (e.g. "podcast-mp3").
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Category for grouping in UIs.
    pub category: PresetCategory,
    /// The output format this preset produces.
    pub format: MediaFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetCategory {
    Audio,
    Video,
    Extract,
}

impl Preset {
    /// Look up a built-in preset by name (case-insensitive).
    pub fn by_name(name: &str) -> Option<&'static Preset> {
        ALL_PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// All built-in presets.
    pub fn all() -> &'static [Preset] {
        &ALL_PRESETS
    }
}

static ALL_PRESETS: [Preset; 14] = [
    // ── Audio presets ──────────────────────────────────────────
    Preset {
        name: "podcast-mp3",
        description: "Podcast-ready MP3: 128 kbps, 44.1 kHz, mono",
        category: PresetCategory::Audio,
        format: MediaFormat {
            container: Container::Mp3,
            audio: Some(AudioSettings {
                codec: AudioCodec::Mp3,
                bitrate: Some(128_000),
                sample_rate: Some(44100),
                channels: Some(1),
            }),
            video: None,
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: true,
        },
    },
    Preset {
        name: "audiobook-m4b",
        description: "Audiobook M4B: AAC 64 kbps, 44.1 kHz, mono",
        category: PresetCategory::Audio,
        format: MediaFormat {
            container: Container::M4b,
            audio: Some(AudioSettings {
                codec: AudioCodec::Aac,
                bitrate: Some(64_000),
                sample_rate: Some(44100),
                channels: Some(1),
            }),
            video: None,
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: true,
        },
    },
    Preset {
        name: "hq-flac",
        description: "High-quality lossless FLAC: 48 kHz, stereo",
        category: PresetCategory::Audio,
        format: MediaFormat {
            container: Container::Flac,
            audio: Some(AudioSettings {
                codec: AudioCodec::Flac,
                bitrate: None,
                sample_rate: Some(48000),
                channels: Some(2),
            }),
            video: None,
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: true,
        },
    },
    Preset {
        name: "opus-voice",
        description: "Opus voice: 32 kbps, 48 kHz, mono",
        category: PresetCategory::Audio,
        format: MediaFormat {
            container: Container::Ogg,
            audio: Some(AudioSettings {
                codec: AudioCodec::Opus,
                bitrate: Some(32_000),
                sample_rate: Some(48000),
                channels: Some(1),
            }),
            video: None,
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: true,
        },
    },
    Preset {
        name: "cd-wav",
        description: "CD-quality WAV: PCM 16-bit, 44.1 kHz, stereo",
        category: PresetCategory::Audio,
        format: MediaFormat {
            container: Container::Wav,
            audio: Some(AudioSettings {
                codec: AudioCodec::Pcm16Le,
                bitrate: None,
                sample_rate: Some(44100),
                channels: Some(2),
            }),
            video: None,
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: true,
        },
    },
    // ── Video presets ──────────────────────────────────────────
    Preset {
        name: "web-mp4",
        description: "Web-optimized MP4: H.264 CRF 23, AAC 128 kbps",
        category: PresetCategory::Video,
        format: MediaFormat {
            container: Container::Mp4,
            audio: Some(AudioSettings {
                codec: AudioCodec::Aac,
                bitrate: Some(128_000),
                sample_rate: None,
                channels: None,
            }),
            video: Some(VideoSettings {
                codec: VideoCodec::H264,
                crf: Some(23),
                width: None,
                height: None,
                fps: None,
                encoder_preset: None,
                pixel_format: None,
            }),
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: false,
        },
    },
    Preset {
        name: "hq-h265",
        description: "High-quality H.265/MKV: CRF 20, Opus 192 kbps",
        category: PresetCategory::Video,
        format: MediaFormat {
            container: Container::Mkv,
            audio: Some(AudioSettings {
                codec: AudioCodec::Opus,
                bitrate: Some(192_000),
                sample_rate: None,
                channels: None,
            }),
            video: Some(VideoSettings {
                codec: VideoCodec::H265,
                crf: Some(20),
                width: None,
                height: None,
                fps: None,
                encoder_preset: None,
                pixel_format: None,
            }),
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: false,
        },
    },
    Preset {
        name: "social-720p",
        description: "Social media 720p: H.264 CRF 23, AAC 128 kbps",
        category: PresetCategory::Video,
        format: MediaFormat {
            container: Container::Mp4,
            audio: Some(AudioSettings {
                codec: AudioCodec::Aac,
                bitrate: Some(128_000),
                sample_rate: None,
                channels: None,
            }),
            video: Some(VideoSettings {
                codec: VideoCodec::H264,
                crf: Some(23),
                width: Some(1280),
                height: Some(720),
                fps: None,
                encoder_preset: None,
                pixel_format: None,
            }),
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: false,
        },
    },
    Preset {
        name: "4k-h265",
        description: "4K H.265: 3840x2160 CRF 18, Opus 256 kbps",
        category: PresetCategory::Video,
        format: MediaFormat {
            container: Container::Mkv,
            audio: Some(AudioSettings {
                codec: AudioCodec::Opus,
                bitrate: Some(256_000),
                sample_rate: None,
                channels: None,
            }),
            video: Some(VideoSettings {
                codec: VideoCodec::H265,
                crf: Some(18),
                width: Some(3840),
                height: Some(2160),
                fps: None,
                encoder_preset: None,
                pixel_format: None,
            }),
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: false,
        },
    },
    Preset {
        name: "webm-vp9",
        description: "WebM VP9: CRF 30, Opus 128 kbps",
        category: PresetCategory::Video,
        format: MediaFormat {
            container: Container::WebM,
            audio: Some(AudioSettings {
                codec: AudioCodec::Opus,
                bitrate: Some(128_000),
                sample_rate: None,
                channels: None,
            }),
            video: Some(VideoSettings {
                codec: VideoCodec::Vp9,
                crf: Some(30),
                width: None,
                height: None,
                fps: None,
                encoder_preset: None,
                pixel_format: None,
            }),
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: false,
        },
    },
    Preset {
        name: "gif",
        description: "Animated GIF: 480px wide, 15 fps",
        category: PresetCategory::Video,
        format: MediaFormat {
            container: Container::Gif,
            audio: None,
            video: Some(VideoSettings {
                codec: VideoCodec::H264, // placeholder — GIF uses internal codec
                crf: None,
                width: Some(480),
                height: None,
                fps: Some(15.0),
                encoder_preset: None,
                pixel_format: None,
            }),
            copy_audio: false,
            copy_video: false,
            no_audio: true,
            no_video: false,
        },
    },
    Preset {
        name: "prores-edit",
        description: "Apple ProRes 422 for editing: PCM audio, MOV",
        category: PresetCategory::Video,
        format: MediaFormat {
            container: Container::Mov,
            audio: Some(AudioSettings {
                codec: AudioCodec::Pcm16Le,
                bitrate: None,
                sample_rate: None,
                channels: None,
            }),
            video: Some(VideoSettings {
                codec: VideoCodec::ProRes,
                crf: None,
                width: None,
                height: None,
                fps: None,
                encoder_preset: None,
                pixel_format: None,
            }),
            copy_audio: false,
            copy_video: false,
            no_audio: false,
            no_video: false,
        },
    },
    // ── Extract presets ────────────────────────────────────────
    Preset {
        name: "thumbnail",
        description: "Extract JPEG thumbnail at first frame",
        category: PresetCategory::Extract,
        format: MediaFormat {
            container: Container::Mp4, // not actually used — thumbnail is special-cased
            audio: None,
            video: None,
            copy_audio: false,
            copy_video: false,
            no_audio: true,
            no_video: false,
        },
    },
    Preset {
        name: "extract-audio",
        description: "Extract audio track (copy codec, strip video)",
        category: PresetCategory::Extract,
        format: MediaFormat {
            container: Container::M4a, // default; actual container chosen at runtime
            audio: None,
            video: None,
            copy_audio: true,
            copy_video: false,
            no_audio: false,
            no_video: true,
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_presets_count() {
        assert_eq!(Preset::all().len(), 14);
    }

    #[test]
    fn lookup_by_name() {
        assert!(Preset::by_name("podcast-mp3").is_some());
        assert!(Preset::by_name("PODCAST-MP3").is_some());
        assert!(Preset::by_name("nonexistent").is_none());
    }

    #[test]
    fn unique_names() {
        let names: Vec<&str> = Preset::all().iter().map(|p| p.name).collect();
        let mut deduped = names.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(names.len(), deduped.len(), "duplicate preset names found");
    }

    #[test]
    fn audio_presets_have_audio_settings() {
        for p in Preset::all().iter().filter(|p| p.category == PresetCategory::Audio) {
            assert!(
                p.format.audio.is_some(),
                "audio preset {} has no audio settings",
                p.name
            );
        }
    }

    #[test]
    fn video_presets_have_video_settings() {
        for p in Preset::all().iter().filter(|p| p.category == PresetCategory::Video) {
            assert!(
                p.format.video.is_some(),
                "video preset {} has no video settings",
                p.name
            );
        }
    }

    #[test]
    fn audio_presets_strip_video() {
        for p in Preset::all().iter().filter(|p| p.category == PresetCategory::Audio) {
            assert!(
                p.format.no_video,
                "audio preset {} should strip video",
                p.name
            );
        }
    }

    #[test]
    fn audio_codec_container_compatibility() {
        for p in Preset::all().iter().filter(|p| p.category == PresetCategory::Audio) {
            if let Some(ref audio) = p.format.audio {
                assert!(
                    p.format.container.supports_audio_codec(audio.codec),
                    "preset {} uses {:?} in {:?} which is incompatible",
                    p.name,
                    audio.codec,
                    p.format.container
                );
            }
        }
    }

    #[test]
    fn video_codec_container_compatibility() {
        for p in Preset::all().iter().filter(|p| p.category == PresetCategory::Video) {
            if let Some(ref video) = p.format.video {
                // Skip GIF — it uses its own internal codec
                if p.format.container == Container::Gif {
                    continue;
                }
                assert!(
                    p.format.container.supports_video_codec(video.codec),
                    "preset {} uses {:?} in {:?} which is incompatible",
                    p.name,
                    video.codec,
                    p.format.container
                );
            }
            if let Some(ref audio) = p.format.audio {
                assert!(
                    p.format.container.supports_audio_codec(audio.codec),
                    "preset {} uses {:?} audio in {:?} which is incompatible",
                    p.name,
                    audio.codec,
                    p.format.container
                );
            }
        }
    }

    #[test]
    fn descriptions_not_empty() {
        for p in Preset::all() {
            assert!(!p.description.is_empty(), "preset {} has empty description", p.name);
        }
    }

    #[test]
    fn serde_category_roundtrip() {
        for cat in &[PresetCategory::Audio, PresetCategory::Video, PresetCategory::Extract] {
            let json = serde_json::to_string(cat).unwrap();
            let parsed: PresetCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(*cat, parsed);
        }
    }
}
