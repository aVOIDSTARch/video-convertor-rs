//! TTS Hub converter plugin: reads audio from stdin, converts, writes to stdout.
//!
//! Options are passed via PLUGIN_OPT_* environment variables:
//!   PLUGIN_OPT_CODEC     - output codec (mp3, aac, flac, opus, wav)
//!   PLUGIN_OPT_BITRATE   - audio bitrate (e.g. "128000" or "128k")
//!   PLUGIN_OPT_SAMPLE_RATE - sample rate in Hz
//!   PLUGIN_OPT_CHANNELS  - number of channels
//!   PLUGIN_OPT_PRESET    - named preset (overrides other options)

use media_convertor_core::codec::AudioCodec;
use media_convertor_core::container::Container;
use media_convertor_core::format::{AudioSettings, MediaFormat};
use media_convertor_core::preset::Preset;
use std::io::{self, Read};

fn main() {
    // Read all input from stdin
    let mut input_bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut input_bytes)
        .expect("failed to read stdin");

    if input_bytes.is_empty() {
        eprintln!("media-convertor-plugin: no input data");
        std::process::exit(1);
    }

    let format = build_format_from_env();

    #[cfg(feature = "ffmpeg")]
    {
        let job = media_convertor_core::transcode::TranscodeJob::new(
            input_bytes,
            media_convertor_core::stream::OutputTarget::Buffer,
            format,
        );

        match media_convertor_core::transcode::transcode_simple(job) {
            Ok(result) => {
                if let Some(bytes) = result.output_bytes {
                    io::stdout()
                        .write_all(&bytes)
                        .expect("failed to write stdout");
                }
            }
            Err(e) => {
                eprintln!("media-convertor-plugin: conversion failed: {e}");
                std::process::exit(1);
            }
        }
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        let _ = format;
        eprintln!("media-convertor-plugin: FFmpeg not available in this build");
        std::process::exit(1);
    }
}

fn build_format_from_env() -> MediaFormat {
    // Check for preset first
    if let Ok(preset_name) = std::env::var("PLUGIN_OPT_PRESET") {
        if !preset_name.is_empty() {
            if let Some(preset) = Preset::by_name(&preset_name) {
                return preset.format.clone();
            }
            eprintln!("media-convertor-plugin: unknown preset '{preset_name}', falling back to defaults");
        }
    }

    let codec_name = std::env::var("PLUGIN_OPT_CODEC").unwrap_or_else(|_| "mp3".to_string());
    let codec = AudioCodec::from_name(&codec_name).unwrap_or_else(|| {
        eprintln!("media-convertor-plugin: unknown codec '{codec_name}', using mp3");
        AudioCodec::Mp3
    });

    let container = default_container_for_codec(codec);
    let mut settings = AudioSettings::new(codec);

    if let Ok(br) = std::env::var("PLUGIN_OPT_BITRATE") {
        if !br.is_empty() {
            if let Some(bps) = parse_bitrate(&br) {
                settings = settings.with_bitrate(bps);
            }
        }
    }

    if let Ok(sr) = std::env::var("PLUGIN_OPT_SAMPLE_RATE") {
        if let Ok(rate) = sr.parse::<u32>() {
            settings = settings.with_sample_rate(rate);
        }
    }

    if let Ok(ch) = std::env::var("PLUGIN_OPT_CHANNELS") {
        if let Ok(channels) = ch.parse::<u16>() {
            settings = settings.with_channels(channels);
        }
    }

    MediaFormat::audio_only(container, settings)
}

fn default_container_for_codec(codec: AudioCodec) -> Container {
    match codec {
        AudioCodec::Mp3 => Container::Mp3,
        AudioCodec::Aac => Container::M4a,
        AudioCodec::Flac => Container::Flac,
        AudioCodec::Vorbis | AudioCodec::Opus => Container::Ogg,
        AudioCodec::Wav | AudioCodec::Pcm16Le | AudioCodec::Pcm24Le | AudioCodec::PcmF32Le => {
            Container::Wav
        }
        AudioCodec::Alac => Container::M4a,
        AudioCodec::Wma => Container::Mkv,
        AudioCodec::Ac3 => Container::Ts,
    }
}

fn parse_bitrate(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('k').or_else(|| s.strip_suffix('K')) {
        num.parse::<u32>().ok().map(|n| n * 1000)
    } else if let Some(num) = s.strip_suffix('m').or_else(|| s.strip_suffix('M')) {
        num.parse::<u32>().ok().map(|n| n * 1_000_000)
    } else {
        s.parse::<u32>().ok()
    }
}
