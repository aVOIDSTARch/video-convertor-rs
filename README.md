# video-convertor-rs

A Rust media conversion library and toolset powered by FFmpeg. Handles audio and video transcoding, probing, normalization, and format conversion. Works standalone for general media tasks and integrates with the Alexandria TTS hub as a `converter` plugin for audio format conversion.

## Project Information

- **Author:** Louis Casinelli Jr
- **Created:** March 14, 2026
- **Language:** Rust
- **Repository:** https://github.com/aVOIDSTARch/video-convertor-rs

## Workspace Structure

```
crates/
  core/       # media-convertor-core — library with codecs, containers, presets, transcode engine
  cli/        # media-convertor-cli — command-line interface with progress bars
  server/     # media-convertor-server — Axum HTTP API with async job tracking
  mcp/        # media-convertor-mcp — MCP server (stdio JSON-RPC)
plugin/       # TTS hub converter plugin (stdin → core → stdout)
```

## Building

FFmpeg is compiled from source via the `bundled` feature flag. The default build compiles without FFmpeg for fast iteration on non-FFmpeg code.

```bash
# Quick build (no FFmpeg, for development)
cargo build --workspace

# Full build with bundled FFmpeg (first build compiles FFmpeg from source)
cargo build --workspace --features bundled

# Run tests
cargo test -p media-convertor-core
```

## CLI Usage

```bash
# Convert with a preset
media-convertor convert input.wav -o output.mp3 --preset podcast-mp3

# Probe media file info
media-convertor probe video.mp4 --json

# List available presets
media-convertor presets

# List supported formats and codecs
media-convertor formats

# Batch conversion from manifest file
media-convertor batch manifest.txt
```

## Presets

| Name | Type | Description |
|------|------|-------------|
| `podcast-mp3` | Audio | MP3 128kbps, 44.1kHz, mono |
| `audiobook-m4b` | Audio | AAC 64kbps, 44.1kHz, mono, M4B container |
| `hq-flac` | Audio | FLAC, 48kHz, stereo |
| `opus-voice` | Audio | Opus 32kbps, 48kHz, mono |
| `cd-wav` | Audio | PCM 16-bit, 44.1kHz, stereo |
| `web-mp4` | Video | H.264 CRF 23, AAC 128k, MP4 |
| `hq-h265` | Video | H.265 CRF 20, Opus 192k, MKV |
| `social-720p` | Video | H.264, 720p, AAC 128k |
| `4k-h265` | Video | H.265, 3840x2160, CRF 18 |
| `webm-vp9` | Video | VP9 CRF 30, Opus 128k, WebM |
| `gif` | Video | GIF, 480px, 15fps |
| `thumbnail` | Extract | JPEG still frame |
| `extract-audio` | Extract | Copy audio, strip video |
| `prores-edit` | Video | ProRes 422, PCM, MOV |

## HTTP Server

```bash
media-convertor-server
# Default: http://127.0.0.1:3000
```

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/convert` | Submit conversion job (multipart: file + preset) |
| GET | `/api/v1/jobs/{id}/status` | Poll job status |
| GET | `/api/v1/jobs/{id}/result` | Download converted file |
| DELETE | `/api/v1/jobs/{id}` | Delete job and files |
| POST | `/api/v1/probe` | Probe media file (multipart) |
| GET | `/api/v1/presets` | List presets |
| GET | `/api/v1/formats` | List formats and codecs |
| GET | `/api/v1/health` | Health check |

## MCP Server

Stdio JSON-RPC server exposing `convert`, `probe`, `extract_audio`, `presets`, and `formats` tools.

```bash
media-convertor-mcp
```

## TTS Hub Plugin

The `plugin/` directory contains a converter plugin for the Alexandria TTS hub. It reads audio from stdin, converts via the core library, and writes to stdout.

```toml
# plugin/plugin.toml
name = "media-convertor"
type = "converter"
```

Options are passed via `PLUGIN_OPT_*` environment variables: `PLUGIN_OPT_CODEC`, `PLUGIN_OPT_BITRATE`, `PLUGIN_OPT_SAMPLE_RATE`, `PLUGIN_OPT_CHANNELS`, `PLUGIN_OPT_PRESET`.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `ffmpeg` | Enable FFmpeg-dependent code (probe, transcode, audio/video ops) |
| `bundled` | Compile FFmpeg from source with x264, x265, opus, vorbis, fdk-aac, dav1d |
| `system-ffmpeg` | Link against system-installed FFmpeg |

## Requirements

- Rust 1.75+
- For `bundled` feature: C compiler, nasm/yasm (FFmpeg build dependencies)
- For `system-ffmpeg` feature: FFmpeg development libraries installed on the system
