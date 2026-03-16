//! media-convertor CLI: convert, probe, presets, formats, batch.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use media_convertor_core::codec::{AudioCodec, VideoCodec};
use media_convertor_core::container::Container;
use media_convertor_core::format::{AudioSettings, MediaFormat, VideoSettings};
use media_convertor_core::preset::Preset;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "media-convertor", version, about = "Media conversion powered by FFmpeg")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, global = true, default_value = "warn")]
    log_level: String,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a media file.
    Convert {
        /// Input file path (or "-" for stdin).
        input: String,
        /// Output file path (or "-" for stdout).
        #[arg(short, long)]
        output: String,
        /// Use a named preset.
        #[arg(long)]
        preset: Option<String>,
        /// Output container format (overrides extension detection).
        #[arg(long)]
        format: Option<String>,
        /// Video codec.
        #[arg(long)]
        video_codec: Option<String>,
        /// Audio codec.
        #[arg(long)]
        audio_codec: Option<String>,
        /// Video CRF (quality).
        #[arg(long)]
        crf: Option<u8>,
        /// Audio bitrate (e.g. "128k", "192000").
        #[arg(long)]
        audio_bitrate: Option<String>,
        /// Video width.
        #[arg(long)]
        width: Option<u32>,
        /// Video height.
        #[arg(long)]
        height: Option<u32>,
        /// Frame rate.
        #[arg(long)]
        fps: Option<f64>,
        /// Sample rate in Hz.
        #[arg(long)]
        sample_rate: Option<u32>,
        /// Audio channels.
        #[arg(long)]
        channels: Option<u16>,
        /// Strip video (audio-only output).
        #[arg(long)]
        no_video: bool,
        /// Strip audio (video-only output).
        #[arg(long)]
        no_audio: bool,
        /// Copy video stream without re-encoding.
        #[arg(long)]
        copy_video: bool,
        /// Copy audio stream without re-encoding.
        #[arg(long)]
        copy_audio: bool,
        /// Start time in seconds.
        #[arg(long)]
        start: Option<f64>,
        /// End time in seconds.
        #[arg(long)]
        end: Option<f64>,
        /// Duration limit in seconds.
        #[arg(long)]
        duration: Option<f64>,
        /// Number of threads (0 = auto).
        #[arg(long, default_value = "0")]
        threads: u32,
    },
    /// Probe a media file and display information.
    Probe {
        /// Input file path.
        input: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List available presets.
    Presets {
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List supported formats and codecs.
    Formats {
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Batch convert from a manifest file.
    Batch {
        /// Path to batch manifest (one job per line: INPUT OUTPUT [--preset NAME | flags]).
        manifest: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        Command::Convert {
            input,
            output,
            preset,
            format,
            video_codec,
            audio_codec,
            crf,
            audio_bitrate,
            width,
            height,
            fps,
            sample_rate,
            channels,
            no_video,
            no_audio,
            copy_video,
            copy_audio,
            start,
            end,
            duration,
            threads,
        } => {
            cmd_convert(
                &input,
                &output,
                preset.as_deref(),
                format.as_deref(),
                video_codec.as_deref(),
                audio_codec.as_deref(),
                crf,
                audio_bitrate.as_deref(),
                width,
                height,
                fps,
                sample_rate,
                channels,
                no_video,
                no_audio,
                copy_video,
                copy_audio,
                start,
                end,
                duration,
                threads,
            )
        }
        Command::Probe { input, json } => cmd_probe(&input, json),
        Command::Presets { json } => cmd_presets(json),
        Command::Formats { json } => cmd_formats(json),
        Command::Batch { manifest } => cmd_batch(&manifest),
    }
}

fn cmd_convert(
    input: &str,
    output: &str,
    preset: Option<&str>,
    format: Option<&str>,
    video_codec: Option<&str>,
    audio_codec: Option<&str>,
    crf: Option<u8>,
    audio_bitrate: Option<&str>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    sample_rate: Option<u32>,
    channels: Option<u16>,
    no_video: bool,
    no_audio: bool,
    copy_video: bool,
    copy_audio: bool,
    start: Option<f64>,
    end: Option<f64>,
    duration: Option<f64>,
    threads: u32,
) -> Result<()> {
    let media_format = if let Some(preset_name) = preset {
        let p = Preset::by_name(preset_name)
            .with_context(|| format!("unknown preset: {preset_name}"))?;
        p.format.clone()
    } else {
        build_format_from_flags(
            output,
            format,
            video_codec,
            audio_codec,
            crf,
            audio_bitrate,
            width,
            height,
            fps,
            sample_rate,
            channels,
            no_video,
            no_audio,
            copy_video,
            copy_audio,
        )?
    };

    // Build the transcode job
    let input_source = media_convertor_core::stream::InputSource::File(PathBuf::from(input));
    let output_target = media_convertor_core::stream::OutputTarget::File(PathBuf::from(output));
    let mut job = media_convertor_core::transcode::TranscodeJob::new(input_source, output_target, media_format);

    if let Some(s) = start {
        job = job.with_start_time(s);
    }
    if let Some(e) = end {
        job = job.with_end_time(e);
    }
    if let Some(d) = duration {
        job = job.with_duration(d);
    }
    job = job.with_threads(threads);

    // Set up progress bar
    let pb = indicatif::ProgressBar::new(100);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% ({msg})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.set_message("converting...");

    #[cfg(feature = "ffmpeg")]
    {
        let mut progress = CliProgress { pb: &pb };
        media_convertor_core::transcode::transcode(job, &mut progress, None)?;
        pb.finish_with_message("done");
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        let _ = (job, pb);
        anyhow::bail!("FFmpeg support not compiled in. Build with --features bundled or --features system-ffmpeg");
    }

    #[cfg(feature = "ffmpeg")]
    Ok(())
}

#[cfg(feature = "ffmpeg")]
struct CliProgress<'a> {
    pb: &'a indicatif::ProgressBar,
}

#[cfg(feature = "ffmpeg")]
impl media_convertor_core::progress::ProgressHandler for CliProgress<'_> {
    fn on_progress(&mut self, event: &media_convertor_core::progress::ProgressEvent) {
        if let Some(pct) = event.percent {
            self.pb.set_position(pct as u64);
        }
        if let Some(speed) = event.speed {
            self.pb.set_message(format!("{speed:.1}x"));
        }
    }

    fn on_complete(&mut self) {
        self.pb.finish_with_message("done");
    }

    fn on_error(&mut self, error: &str) {
        self.pb.abandon_with_message(format!("error: {error}"));
    }
}

fn build_format_from_flags(
    output: &str,
    format: Option<&str>,
    video_codec: Option<&str>,
    audio_codec: Option<&str>,
    crf: Option<u8>,
    audio_bitrate: Option<&str>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    sample_rate: Option<u32>,
    channels: Option<u16>,
    no_video: bool,
    no_audio: bool,
    copy_video: bool,
    copy_audio: bool,
) -> Result<MediaFormat> {
    // Determine container
    let container = if let Some(fmt) = format {
        Container::from_name(fmt)
            .with_context(|| format!("unknown format: {fmt}"))?
    } else {
        let path = PathBuf::from(output);
        Container::from_path(&path)
            .with_context(|| format!("cannot determine format from extension: {output}"))?
    };

    // Parse codecs
    let vc = video_codec
        .map(|s| VideoCodec::from_name(s).with_context(|| format!("unknown video codec: {s}")))
        .transpose()?;
    let ac = audio_codec
        .map(|s| AudioCodec::from_name(s).with_context(|| format!("unknown audio codec: {s}")))
        .transpose()?;

    // Parse audio bitrate (supports "128k" or raw number)
    let bitrate = audio_bitrate.map(parse_bitrate).transpose()?;

    // Build video settings
    let video = if !no_video && !copy_video {
        vc.map(|codec| {
            let mut vs = VideoSettings::new(codec);
            if let Some(c) = crf {
                vs = vs.with_crf(c);
            }
            if let (Some(w), Some(h)) = (width, height) {
                vs = vs.with_resolution(w, h);
            }
            if let Some(f) = fps {
                vs = vs.with_fps(f);
            }
            vs
        })
    } else {
        None
    };

    // Build audio settings
    let audio = if !no_audio && !copy_audio {
        ac.map(|codec| {
            let mut aus = AudioSettings::new(codec);
            if let Some(br) = bitrate {
                aus = aus.with_bitrate(br);
            }
            if let Some(sr) = sample_rate {
                aus = aus.with_sample_rate(sr);
            }
            if let Some(ch) = channels {
                aus = aus.with_channels(ch);
            }
            aus
        })
    } else {
        None
    };

    Ok(MediaFormat {
        container,
        audio,
        video,
        copy_audio,
        copy_video,
        no_audio,
        no_video,
    })
}

fn parse_bitrate(s: &str) -> Result<u32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('k').or_else(|| s.strip_suffix('K')) {
        let n: u32 = num.parse().context("invalid bitrate number")?;
        Ok(n * 1000)
    } else if let Some(num) = s.strip_suffix('m').or_else(|| s.strip_suffix('M')) {
        let n: u32 = num.parse().context("invalid bitrate number")?;
        Ok(n * 1_000_000)
    } else {
        s.parse::<u32>().context("invalid bitrate")
    }
}

fn cmd_probe(input: &str, json: bool) -> Result<()> {
    #[cfg(feature = "ffmpeg")]
    {
        let info = media_convertor_core::detect::probe_file(std::path::Path::new(input))
            .with_context(|| format!("failed to probe: {input}"))?;

        if json {
            println!("{}", serde_json::to_string_pretty(&info)?);
        } else {
            println!("Format: {} ({})", info.format_name, info.container.map(|c| c.extension()).unwrap_or("unknown"));
            if let Some(dur) = info.duration {
                println!("Duration: {:.1}s", dur.as_secs_f64());
            }
            if let Some(br) = info.bitrate {
                println!("Bitrate: {} kbps", br / 1000);
            }
            println!("Streams: {}", info.streams.len());
            for s in &info.streams {
                match s.stream_type {
                    media_convertor_core::detect::StreamType::Video => {
                        println!(
                            "  [{}] Video: {} {}x{} {:.1} fps",
                            s.index,
                            s.codec_name,
                            s.width.unwrap_or(0),
                            s.height.unwrap_or(0),
                            s.fps.unwrap_or(0.0),
                        );
                    }
                    media_convertor_core::detect::StreamType::Audio => {
                        println!(
                            "  [{}] Audio: {} {} Hz {} ch",
                            s.index,
                            s.codec_name,
                            s.sample_rate.unwrap_or(0),
                            s.channels.unwrap_or(0),
                        );
                    }
                    _ => {
                        println!("  [{}] {:?}: {}", s.index, s.stream_type, s.codec_name);
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        let _ = (input, json);
        anyhow::bail!("FFmpeg support not compiled in. Build with --features bundled or --features system-ffmpeg");
    }
}

fn cmd_presets(json: bool) -> Result<()> {
    let presets = Preset::all();
    if json {
        println!("{}", serde_json::to_string_pretty(presets)?);
    } else {
        println!("{:<20} {:<10} {}", "NAME", "CATEGORY", "DESCRIPTION");
        println!("{}", "-".repeat(70));
        for p in presets {
            println!("{:<20} {:<10} {}", p.name, format!("{:?}", p.category).to_lowercase(), p.description);
        }
    }
    Ok(())
}

fn cmd_formats(json: bool) -> Result<()> {
    if json {
        #[derive(serde::Serialize)]
        struct FormatsOutput {
            containers: Vec<ContainerInfo>,
            audio_codecs: Vec<CodecInfo>,
            video_codecs: Vec<CodecInfo>,
        }
        #[derive(serde::Serialize)]
        struct ContainerInfo {
            name: String,
            extension: String,
            mime_type: String,
            supports_video: bool,
            supports_audio: bool,
        }
        #[derive(serde::Serialize)]
        struct CodecInfo {
            name: String,
            display_name: String,
            encoder: String,
        }

        let out = FormatsOutput {
            containers: Container::all()
                .iter()
                .map(|c| ContainerInfo {
                    name: format!("{:?}", c),
                    extension: c.extension().to_string(),
                    mime_type: c.mime_type().to_string(),
                    supports_video: c.supports_video(),
                    supports_audio: c.supports_audio(),
                })
                .collect(),
            audio_codecs: AudioCodec::all()
                .iter()
                .map(|c| CodecInfo {
                    name: format!("{:?}", c),
                    display_name: c.display_name().to_string(),
                    encoder: c.ffmpeg_encoder().to_string(),
                })
                .collect(),
            video_codecs: VideoCodec::all()
                .iter()
                .map(|c| CodecInfo {
                    name: format!("{:?}", c),
                    display_name: c.display_name().to_string(),
                    encoder: c.ffmpeg_encoder().to_string(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("CONTAINERS:");
        println!("{:<10} {:<25} {:<8} {}", "EXT", "MIME", "VIDEO", "AUDIO");
        for c in Container::all() {
            println!(
                "{:<10} {:<25} {:<8} {}",
                c.extension(),
                c.mime_type(),
                if c.supports_video() { "yes" } else { "no" },
                if c.supports_audio() { "yes" } else { "no" },
            );
        }
        println!("\nAUDIO CODECS:");
        println!("{:<15} {:<25} {}", "NAME", "DISPLAY", "ENCODER");
        for c in AudioCodec::all() {
            println!("{:<15} {:<25} {}", format!("{:?}", c), c.display_name(), c.ffmpeg_encoder());
        }
        println!("\nVIDEO CODECS:");
        println!("{:<15} {:<25} {}", "NAME", "DISPLAY", "ENCODER");
        for c in VideoCodec::all() {
            println!("{:<15} {:<25} {}", format!("{:?}", c), c.display_name(), c.ffmpeg_encoder());
        }
    }
    Ok(())
}

fn cmd_batch(manifest: &str) -> Result<()> {
    let content = std::fs::read_to_string(manifest)
        .with_context(|| format!("failed to read manifest: {manifest}"))?;

    let lines: Vec<&str> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    println!("Batch: {} jobs", lines.len());

    for (i, line) in lines.iter().enumerate() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            eprintln!("  [{}] skipping invalid line: {}", i + 1, line);
            continue;
        }

        let input = parts[0];
        let output = parts[1];
        let preset_name = parts.get(2).copied();

        println!("  [{}/{}] {} -> {} (preset: {})",
            i + 1, lines.len(), input, output,
            preset_name.unwrap_or("auto"));

        if let Some(name) = preset_name {
            cmd_convert(input, output, Some(name), None, None, None, None, None, None, None, None, None, None, false, false, false, false, None, None, None, 0)?;
        } else {
            cmd_convert(input, output, None, None, None, None, None, None, None, None, None, None, None, false, false, false, false, None, None, None, 0)?;
        }
    }

    println!("Batch complete.");
    Ok(())
}
