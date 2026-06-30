//! TTS Hub converter plugin: reads audio from stdin, converts, writes to stdout.
//!
//! Options are passed via PLUGIN_OPT_* environment variables:
//!   PLUGIN_OPT_CODEC       - output codec (mp3, aac, flac, opus, wav)
//!   PLUGIN_OPT_BITRATE     - audio bitrate (e.g. "128000" or "128k")
//!   PLUGIN_OPT_SAMPLE_RATE - sample rate in Hz
//!   PLUGIN_OPT_CHANNELS    - number of channels
//!   PLUGIN_OPT_PRESET      - named preset (overrides other options)
//!
//! Conversion runs the system ffmpeg via the core engine, bouncing through temp files
//! (ffmpeg needs seekable I/O for most containers).

use media_convertor_core::codec::AudioCodec;
use media_convertor_core::container::Container;
use media_convertor_core::ffmpeg::command::{self, Trim};
use media_convertor_core::format::{AudioSettings, MediaFormat};
use media_convertor_core::operation::parse_bitrate;
use media_convertor_core::progress::NoProgress;
use media_convertor_core::{Config, Engine, Preset};
use std::io::{self, Read, Write};

fn main() {
    let mut input_bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut input_bytes)
        .expect("failed to read stdin");

    if input_bytes.is_empty() {
        eprintln!("media-convertor-plugin: no input data");
        std::process::exit(1);
    }

    if let Err(e) = run(&input_bytes) {
        eprintln!("media-convertor-plugin: {e}");
        std::process::exit(1);
    }
}

fn run(input_bytes: &[u8]) -> Result<(), String> {
    let format = build_format_from_env();
    let engine = Engine::new(&Config::default()).map_err(|e| e.to_string())?;

    let tmp = std::env::temp_dir();
    let id = uuid::Uuid::new_v4();
    let tmp_in = tmp.join(format!("mcplugin-{id}.in"));
    let tmp_out = tmp.join(format!("mcplugin-{id}.{}", format.container.extension()));

    std::fs::write(&tmp_in, input_bytes).map_err(|e| e.to_string())?;

    let cmd = command::transcode_args(&tmp_in, &tmp_out, &format, &Trim::default());
    let result = engine.run(&cmd, None, &mut NoProgress, None);
    let _ = std::fs::remove_file(&tmp_in);

    let out = match result {
        Ok(()) => std::fs::read(&tmp_out).map_err(|e| e.to_string())?,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_out);
            return Err(format!("conversion failed: {e}"));
        }
    };
    let _ = std::fs::remove_file(&tmp_out);

    io::stdout().write_all(&out).map_err(|e| e.to_string())?;
    Ok(())
}

fn build_format_from_env() -> MediaFormat {
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
            if let Ok(bps) = parse_bitrate(&br) {
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
