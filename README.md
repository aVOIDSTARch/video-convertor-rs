# video-convertor-rs

A comprehensive, secure Rust **shell over FFmpeg**. One importable library backs two
binaries — a **CLI control plane** and a **thin, locked-down HTTP API** — and everything
flows through a **reusable, persistent request queue**.

Rather than hand-maintaining codec/format lists, the engine wraps the system
`ffmpeg`/`ffprobe` binaries and **discovers their full capability set at runtime** (every
encoder, decoder, filter, muxer, demuxer, pixel format, and protocol the installed build
supports). Upgrade ffmpeg and the new capabilities appear automatically.

## Project Information

* **Author:** Louis Casinelli Jr
* **Language:** Rust (edition 2021)
* **Repository:** <https://github.com/aVOIDSTARch/video-convertor-rs>

## Architecture

``` bash
            ┌──────────────────────── media-convertor-core (lib.rs) ───────────────────────┐
            │  ffmpeg/  — locate · capabilities · command builder · run (progress/timeout)  │
            │  operation + dispatch — structured ops + gated raw, routed from requests      │
CLI  ───────┤  api_queue.rs — reusable, persistent queue of universal HTTP-shaped requests  │
API  ───────┤  security — path confinement · protocol denylist · raw allowlist · tokens     │
            └──────────────────────────────────────────────────────────────────────────────┘
                                   │ FfmpegHandler (QueueHandler)
                                   ▼
                         ffmpeg / ffprobe subprocess
```

* **CLI = control plane.** Runs every operation locally *and* manages the server
  (`server start|stop|status`, foreground/background, verbose/quiet). With `--server
  <URL>` it submits the exact same request to a remote API instead.
* **API = ffmpeg-only.** It exposes conversion operations and job state — never server or
  admin controls. Each request is enqueued and processed by the shared queue.
* **`api_queue.rs` is self-contained** (no ffmpeg dependency): its work item is a
  `UniversalRequest { method, path, query, headers, body, attachments }`, persisted as
  JSON. Drop the module into any project and supply your own `QueueHandler`.

## Workspace

``` bash
crates/
  core/       # media-convertor-core — the library (engine, queue, ops, security)
  cli/        # media-convertor       — control-plane CLI (local + remote + server mgmt)
  server/     # media-convertor-server — thin, locked-down HTTP API
  mcp/        # media-convertor-mcp    — MCP stdio JSON-RPC server
plugin/       # media-convertor-plugin — TTS hub converter (stdin → ffmpeg → stdout)
```

## Requirements

* Rust 1.75+
* **FFmpeg 6.0+ installed and on `PATH`** (provides `ffmpeg` and `ffprobe`). No build-time
  FFmpeg linking — the engine shells out, so any system build works:
  `brew install ffmpeg` / `apt install ffmpeg`.

## Building

```bash
cargo build --workspace          # builds everything; no special features needed
```

## Testing

```bash
cargo test --workspace           # unit + integration tests across every crate
```

The suite spans core unit tests plus integration tests that drive each surface as a
subprocess — the CLI binary, the HTTP router, the MCP stdio server, and the TTS plugin.
Tests that need a real conversion **self-skip when `ffmpeg`/`ffprobe` are not on `PATH`**
(they print `SKIP: ffmpeg/ffprobe not found`), so the suite stays green in minimal
environments while exercising real ffmpeg when it is available.

## CLI usage

```bash
# Convert with a preset, or with explicit options
media-convertor convert in.wav -o out.mp3 --preset podcast-mp3
media-convertor convert in.mov -o out.mkv --video-codec h265 --crf 22 \
                                          --audio-codec opus --audio-bitrate 128k

# Probe, presets, and runtime-discovered capabilities
media-convertor probe video.mp4
media-convertor presets
media-convertor capabilities encoders     # all|encoders|decoders|filters|muxers|demuxers|...

# More operations
media-convertor extract-audio movie.mkv -o track.m4a --codec aac
media-convertor thumbnail movie.mkv -o thumb.jpg --time 5 --width 320
media-convertor filter in.mp4 -o out.mp4 --graph "scale=640:-2,fps=30"
media-convertor concat -o joined.mkv a.mkv b.mkv c.mkv
media-convertor raw in.mp4 -o out.mp4 --enable-raw -- -vf hue=s=0 -c:v libx264

# Batch (manifest lines: INPUT OUTPUT [PRESET])
media-convertor batch jobs.txt

# Talk to a remote server instead of running locally (identical request, dual-mode)
media-convertor --server http://host:3400 convert in.wav -o out.mp3 --preset podcast-mp3
```

### Managing the server from the CLI

```bash
media-convertor server start --workers 4            # background daemon (pidfile)
media-convertor server start --foreground -v        # foreground, verbose
media-convertor server start --host 0.0.0.0 --token "$TOKEN"   # remote bind (token required)
media-convertor server start --enable-raw           # allow the gated raw op
media-convertor server status
media-convertor server stop
```

## HTTP API

Operations only — no admin/lifecycle endpoints. File-producing ops and probe are queued;
capabilities/presets are answered inline.

| Method | Path | Description |
| -------- | ------ | ------------- |
| GET | `/api/v1/health` | Health check (no auth) |
| GET | `/api/v1/capabilities?kind=` | Discovered capabilities |
| GET | `/api/v1/presets` | Built-in presets |
| POST | `/api/v1/convert` | Transcode (multipart: `file` + JSON `request`) |
| POST | `/api/v1/extract-audio` | Extract audio track |
| POST | `/api/v1/thumbnail` | Single-frame thumbnail |
| POST | `/api/v1/filter` | Apply a video filtergraph |
| POST | `/api/v1/concat` | Concatenate multiple `file*` parts |
| POST | `/api/v1/probe` | Probe a file |
| POST | `/api/v1/raw` | Gated raw passthrough (if enabled) |
| GET | `/api/v1/jobs/{id}/status` | Poll status/progress |
| GET | `/api/v1/jobs/{id}/result` | Download output (or inline JSON) |
| DELETE | `/api/v1/jobs/{id}` | Delete job + files |

`request` is a JSON object matching the operation (for `convert` it is the full
`ConvertRequest`: `preset`, `format`, codecs, `crf`, `audio_bitrate`, `width/height`,
`fps`, `sample_rate`, `channels`, `encoder_preset`, `pixel_format`, copy/strip flags,
`start/end/duration`).

```bash
curl -F file=@in.wav -F 'request={"preset":"podcast-mp3"}' \
     http://127.0.0.1:3400/api/v1/convert
# → {"job_id":"…","status":"queued"} ; then poll /status and GET /result
```

### Security

* **Binds `127.0.0.1` by default.** Any non-loopback bind **requires** a bearer token
  (`--token` / `MEDIA_CONVERTOR_TOKEN`); the server refuses otherwise.
* No shell is ever invoked — ffmpeg runs from an argument vector with
  `-nostdin -protocol_whitelist file,crypto`.
* Uploads are size-limited, filenames sanitized, and all I/O confined to managed work
  dirs (traversal/symlink escapes rejected).
* Per-job wall-clock timeout kills runaway processes.
* The **raw passthrough is disabled by default**; when enabled it still runs under an
  argument allowlist, protocol denylist, and I/O confinement.

### Configuration (env vars / CLI flags)

| Env | Flag | Default |
| ----- | ------ | --------- |
| `MEDIA_CONVERTOR_HOST` | `--host` | `127.0.0.1` |
| `MEDIA_CONVERTOR_PORT` | `--port` | `3400` |
| `MEDIA_CONVERTOR_WORKERS` | `--workers` | `2` |
| `MEDIA_CONVERTOR_DATA` | `--work-dir` | `~/.media-convertor` |
| `MEDIA_CONVERTOR_TIMEOUT` | — | `3600` (seconds, 0 = none) |
| `MEDIA_CONVERTOR_TOKEN` | `--token` | none |
| `MEDIA_CONVERTOR_RAW` | `--enable-raw` | off |

The persistent queue stores each job as `jobs/{id}.json`; on restart, in-flight jobs are
re-queued (nothing is lost), and uploaded inputs live under `uploads/`, outputs under
`output/`.

## Using the library

```rust
use media_convertor_core::{Config, Engine};
use media_convertor_core::ffmpeg::command::{transcode_args, Trim};
use media_convertor_core::operation::ConvertRequest;
use media_convertor_core::progress::NoProgress;

let engine = Engine::new(&Config::default())?;  // locates ffmpeg, discovers caps
let fmt = ConvertRequest { preset: Some("web-mp4".into()), ..Default::default() }
    .build_format(None)?;
let args = transcode_args("in.mov".as_ref(), "out.mp4".as_ref(), &fmt, &Trim::default());
engine.run(&args, None, &mut NoProgress, None)?;
```

The reusable [`api_queue`] module can back any project that needs a durable queue of
HTTP-shaped requests — implement `QueueHandler` for your own work.

## MCP server

Stdio JSON-RPC exposing every non-admin operation as a tool — `convert`,
`extract_audio`, `thumbnail`, `filter`, `concat`, `raw`, `probe`, `presets`, and
`capabilities` — mirroring the CLI/HTTP surface. Server/lifecycle controls are never
exposed. Each tool dispatches through the same shared handler the CLI's local mode and the
HTTP server's queue workers use.

```bash
media-convertor-mcp        # speaks JSON-RPC on stdin/stdout
```

Tools take absolute file paths and write the result to the requested `output_path` (inline
ops — `probe`, `presets`, `capabilities` — return JSON in the tool result). The `convert`
tool accepts the full `ConvertRequest` field set (see the HTTP API above).

```jsonc
// → list tools
{"jsonrpc":"2.0","id":1,"method":"tools/list"}
// → transcode a file
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
  "name":"convert",
  "arguments":{"input_path":"/in.wav","output_path":"/out.mp3","preset":"podcast-mp3"}}}
```

Configuration mirrors the other surfaces via env vars: `MEDIA_CONVERTOR_DATA` sets the
managed work directory, and the gated `raw` passthrough is permitted only when
`MEDIA_CONVERTOR_RAW` is set (otherwise `raw` returns a security error and writes nothing).

## TTS hub plugin

`plugin/` reads audio on stdin, converts via the engine, and writes the result to stdout —
a stream filter with no files or flags. Options come from `PLUGIN_OPT_*` env vars:
`CODEC` (mp3, aac, flac, opus, wav, …), `BITRATE` (`"128k"` or `"128000"`), `SAMPLE_RATE`,
`CHANNELS`, and `PRESET` (a named preset that overrides the others). An unknown codec falls
back to mp3 with a warning on stderr.

```bash
PLUGIN_OPT_CODEC=mp3 PLUGIN_OPT_BITRATE=128k \
  media-convertor-plugin < speech.wav > speech.mp3
```
