//! media-convertor MCP server: stdio JSON-RPC exposing every non-admin ffmpeg
//! operation (convert, extract_audio, thumbnail, filter, concat, raw, probe, presets,
//! capabilities). Server/lifecycle controls are intentionally never exposed.
//!
//! Follows the file-processor-mcp pattern: custom stdio JSON-RPC, not the MCP SDK.
//! Every tool dispatches through the shared [`FfmpegHandler`] — the same code path the
//! CLI's local mode and the HTTP server's queue workers use.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("media-convertor MCP server starting");

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("stdin read error: {e}");
                break;
            }
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = json_rpc_error(None, -32700, &format!("parse error: {e}"));
                let _ = writeln!(out, "{}", serde_json::to_string(&err).unwrap());
                continue;
            }
        };

        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => handle_initialize(id.clone()),
            "tools/list" => handle_tools_list(id.clone()),
            "tools/call" => handle_tools_call(id.clone(), &params),
            "shutdown" => {
                let resp = json_rpc_result(id, json!({"status": "ok"}));
                let _ = writeln!(out, "{}", serde_json::to_string(&resp).unwrap());
                break;
            }
            _ => json_rpc_error(id, -32601, &format!("unknown method: {method}")),
        };

        let _ = writeln!(out, "{}", serde_json::to_string(&response).unwrap());
        let _ = out.flush();
    }
}

fn handle_initialize(id: Option<Value>) -> Value {
    json_rpc_result(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "media-convertor-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list(id: Option<Value>) -> Value {
    json_rpc_result(
        id,
        json!({
            "tools": [
                {
                    "name": "convert",
                    "description": "Convert a media file to another format (full transcode)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input_path": { "type": "string", "description": "Path to input file" },
                            "output_path": { "type": "string", "description": "Path to output file" },
                            "preset": { "type": "string", "description": "Preset name (e.g. podcast-mp3, web-mp4)" },
                            "format": { "type": "string", "description": "Target container (e.g. mp4); defaults to the output extension" },
                            "audio_codec": { "type": "string", "description": "Audio codec name" },
                            "video_codec": { "type": "string", "description": "Video codec name" },
                            "audio_bitrate": { "type": "string", "description": "Audio bitrate, e.g. \"128k\" or \"192000\"" },
                            "crf": { "type": "integer", "description": "Video CRF quality (lower = better)" },
                            "width": { "type": "integer", "description": "Output width in pixels" },
                            "height": { "type": "integer", "description": "Output height in pixels" },
                            "fps": { "type": "number", "description": "Output frame rate" },
                            "sample_rate": { "type": "integer", "description": "Audio sample rate in Hz" },
                            "channels": { "type": "integer", "description": "Audio channel count" },
                            "encoder_preset": { "type": "string", "description": "Encoder speed/quality preset (e.g. slow, medium, fast)" },
                            "pixel_format": { "type": "string", "description": "Output pixel format (e.g. yuv420p)" },
                            "no_video": { "type": "boolean", "description": "Drop the video stream" },
                            "no_audio": { "type": "boolean", "description": "Drop the audio stream" },
                            "copy_video": { "type": "boolean", "description": "Stream-copy video (no re-encode)" },
                            "copy_audio": { "type": "boolean", "description": "Stream-copy audio (no re-encode)" },
                            "start": { "type": "number", "description": "Trim start in seconds" },
                            "end": { "type": "number", "description": "Trim end in seconds" },
                            "duration": { "type": "number", "description": "Trim duration in seconds" }
                        },
                        "required": ["input_path", "output_path"]
                    }
                },
                {
                    "name": "extract_audio",
                    "description": "Extract audio track from a video file",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input_path": { "type": "string", "description": "Path to input video" },
                            "output_path": { "type": "string", "description": "Path to output audio file" },
                            "codec": { "type": "string", "description": "Re-encode with this audio codec instead of copying" }
                        },
                        "required": ["input_path", "output_path"]
                    }
                },
                {
                    "name": "thumbnail",
                    "description": "Extract a single thumbnail frame from a video",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input_path": { "type": "string", "description": "Path to input video" },
                            "output_path": { "type": "string", "description": "Path to output image file" },
                            "time": { "type": "number", "description": "Timestamp in seconds (default 0)" },
                            "width": { "type": "integer", "description": "Output width in pixels (height auto)" }
                        },
                        "required": ["input_path", "output_path"]
                    }
                },
                {
                    "name": "filter",
                    "description": "Apply a single ffmpeg video filtergraph",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input_path": { "type": "string", "description": "Path to input file" },
                            "output_path": { "type": "string", "description": "Path to output file" },
                            "graph": { "type": "string", "description": "Filtergraph, e.g. \"scale=640:-2,fps=30\"" },
                            "format": { "type": "string", "description": "Target container; defaults to the output extension" }
                        },
                        "required": ["input_path", "output_path", "graph"]
                    }
                },
                {
                    "name": "concat",
                    "description": "Concatenate multiple inputs into one output (2 or more)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input_paths": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Input files, in order (2 or more)"
                            },
                            "output_path": { "type": "string", "description": "Path to output file" },
                            "format": { "type": "string", "description": "Target container; defaults to the output extension" }
                        },
                        "required": ["input_paths", "output_path"]
                    }
                },
                {
                    "name": "raw",
                    "description": "Gated raw ffmpeg passthrough. Disabled unless MEDIA_CONVERTOR_RAW is set; still sandboxed under an argument allowlist and I/O confinement",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input_path": { "type": "string", "description": "Path to input file" },
                            "output_path": { "type": "string", "description": "Path to output file" },
                            "args": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "ffmpeg arguments inserted between input and output"
                            }
                        },
                        "required": ["input_path", "output_path", "args"]
                    }
                },
                {
                    "name": "probe",
                    "description": "Probe a media file and return stream info, codecs, duration",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to media file" }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "presets",
                    "description": "List available conversion presets",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "capabilities",
                    "description": "List the installed ffmpeg's capabilities (encoders, decoders, filters, muxers, demuxers), discovered at runtime",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "description": "all|encoders|decoders|filters|muxers|demuxers" }
                        }
                    }
                }
            ]
        }),
    )
}

fn handle_tools_call(id: Option<Value>, params: &Value) -> Value {
    let tool_name = params
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    let result = match tool_name {
        "convert" => tool_convert(&arguments),
        "extract_audio" => tool_extract_audio(&arguments),
        "thumbnail" => tool_thumbnail(&arguments),
        "filter" => tool_filter(&arguments),
        "concat" => tool_concat(&arguments),
        "raw" => tool_raw(&arguments),
        "probe" => tool_probe(&arguments),
        "presets" => tool_presets(&arguments),
        "capabilities" => tool_capabilities(&arguments),
        _ => Err(format!("unknown tool: {tool_name}")),
    };

    match result {
        Ok(content) => json_rpc_result(
            id,
            json!({
                "content": [{ "type": "text", "text": content }]
            }),
        ),
        Err(e) => json_rpc_result(
            id,
            json!({
                "content": [{ "type": "text", "text": format!("Error: {e}") }],
                "isError": true
            }),
        ),
    }
}

use media_convertor_core::api_queue::{Attachment, JobResult, ProgressReporter, UniversalRequest};
use media_convertor_core::{Config, Engine, FfmpegHandler};
use std::path::Path;
use std::sync::OnceLock;
use uuid::Uuid;

/// Lazily-built shared handler. Locates ffmpeg, discovers capabilities, and prepares the
/// managed work dirs once. Every tool dispatches through this — the same code path the
/// CLI's local mode and the HTTP server's queue workers use. The gated raw passthrough is
/// only permitted when `MEDIA_CONVERTOR_RAW` is set.
fn handler() -> Result<&'static FfmpegHandler, String> {
    static HANDLER: OnceLock<FfmpegHandler> = OnceLock::new();
    if let Some(h) = HANDLER.get() {
        return Ok(h);
    }
    let mut config = Config::default();
    if let Ok(dir) = std::env::var("MEDIA_CONVERTOR_DATA") {
        if !dir.is_empty() {
            config.work_dir = dir.into();
        }
    }
    config.ensure_dirs().map_err(|e| e.to_string())?;
    let engine = Engine::new(&config).map_err(|e| e.to_string())?;
    let raw_enabled = std::env::var("MEDIA_CONVERTOR_RAW")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let handler = FfmpegHandler::new(engine, config.output_dir(), raw_enabled);
    Ok(HANDLER.get_or_init(|| handler))
}

/// No-op progress reporter (MCP tools run synchronously and report only on completion).
struct NoReporter;
impl ProgressReporter for NoReporter {
    fn report(&self, _progress: f64) {}
}

/// Fetch a required string argument.
fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing {key}"))
}

/// An [`Attachment`] pointing directly at a user-provided local file (no copy).
fn local_attachment(field: &str, path: &Path) -> Result<Attachment, String> {
    if !path.exists() {
        return Err(format!("input file not found: {}", path.display()));
    }
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input")
        .to_string();
    Ok(Attachment {
        field: field.to_string(),
        filename,
        path: path.to_path_buf(),
    })
}

/// Dispatch a request through the shared handler. For file-producing ops, move the
/// managed output to `output`; inline ops (probe/presets/capabilities) return JSON in the
/// [`JobResult`] body.
fn run_local(request: UniversalRequest, output: Option<&Path>) -> Result<JobResult, String> {
    let result = handler()?
        .execute(Uuid::new_v4(), &request, &NoReporter)
        .map_err(|e| e.to_string())?;
    if let (Some(produced), Some(dest)) = (result.output_path.as_ref(), output) {
        move_file(produced, dest)?;
    }
    Ok(result)
}

/// Move a file, falling back to copy+remove across filesystem boundaries.
fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(from);
            Ok(())
        }
    }
}

fn tool_convert(args: &Value) -> Result<String, String> {
    let input_path = str_arg(args, "input_path")?;
    let output_path = str_arg(args, "output_path")?;

    let mut body = serde_json::Map::new();
    for key in [
        "preset", "format", "video_codec", "audio_codec", "crf", "width", "height", "fps",
        "sample_rate", "channels", "encoder_preset", "pixel_format", "no_video", "no_audio",
        "copy_video", "copy_audio", "start", "end", "duration",
    ] {
        if let Some(v) = args.get(key) {
            if !v.is_null() {
                body.insert(key.to_string(), v.clone());
            }
        }
    }
    // `audio_bitrate` is a string ("128k"); accept a bare number too, for convenience.
    if let Some(v) = args.get("audio_bitrate") {
        match v {
            Value::String(s) => {
                body.insert("audio_bitrate".to_string(), json!(s));
            }
            Value::Number(n) => {
                body.insert("audio_bitrate".to_string(), json!(n.to_string()));
            }
            _ => {}
        }
    }
    // Default the container to the output extension when no preset/format was given.
    if !body.contains_key("preset") && !body.contains_key("format") {
        if let Some(ext) = Path::new(output_path).extension().and_then(|e| e.to_str()) {
            body.insert("format".to_string(), json!(ext));
        }
    }

    let req = UniversalRequest::post("convert")
        .with_body(Value::Object(body))
        .with_attachment(local_attachment("file", Path::new(input_path))?);
    let result = run_local(req, Some(Path::new(output_path)))?;
    Ok(format!("Converted {input_path} -> {output_path} ({} bytes)", result.size))
}

fn tool_extract_audio(args: &Value) -> Result<String, String> {
    let input_path = str_arg(args, "input_path")?;
    let output_path = str_arg(args, "output_path")?;

    let mut body = serde_json::Map::new();
    if let Some(codec) = args.get("codec").and_then(Value::as_str) {
        body.insert("codec".to_string(), json!(codec));
    }
    // Pick the muxer from the requested output extension (default is m4a).
    if let Some(ext) = Path::new(output_path).extension().and_then(|e| e.to_str()) {
        body.insert("format".to_string(), json!(ext));
    }

    let req = UniversalRequest::post("extract-audio")
        .with_body(Value::Object(body))
        .with_attachment(local_attachment("file", Path::new(input_path))?);
    let result = run_local(req, Some(Path::new(output_path)))?;
    Ok(format!("Extracted audio: {input_path} -> {output_path} ({} bytes)", result.size))
}

fn tool_thumbnail(args: &Value) -> Result<String, String> {
    let input_path = str_arg(args, "input_path")?;
    let output_path = str_arg(args, "output_path")?;

    let mut body = serde_json::Map::new();
    for key in ["time", "width"] {
        if let Some(v) = args.get(key) {
            if !v.is_null() {
                body.insert(key.to_string(), v.clone());
            }
        }
    }

    let req = UniversalRequest::post("thumbnail")
        .with_body(Value::Object(body))
        .with_attachment(local_attachment("file", Path::new(input_path))?);
    let result = run_local(req, Some(Path::new(output_path)))?;
    Ok(format!("Thumbnail: {input_path} -> {output_path} ({} bytes)", result.size))
}

fn tool_filter(args: &Value) -> Result<String, String> {
    let input_path = str_arg(args, "input_path")?;
    let output_path = str_arg(args, "output_path")?;
    let graph = str_arg(args, "graph")?;

    let mut body = serde_json::Map::new();
    body.insert("graph".to_string(), json!(graph));
    let format = args
        .get("format")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            Path::new(output_path)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_string)
        });
    if let Some(format) = format {
        body.insert("format".to_string(), json!(format));
    }

    let req = UniversalRequest::post("filter")
        .with_body(Value::Object(body))
        .with_attachment(local_attachment("file", Path::new(input_path))?);
    let result = run_local(req, Some(Path::new(output_path)))?;
    Ok(format!("Filtered {input_path} -> {output_path} ({} bytes)", result.size))
}

fn tool_concat(args: &Value) -> Result<String, String> {
    let inputs = args
        .get("input_paths")
        .and_then(Value::as_array)
        .ok_or("missing input_paths")?;
    if inputs.len() < 2 {
        return Err("concat requires at least 2 input_paths".to_string());
    }
    let output_path = str_arg(args, "output_path")?;

    let mut body = serde_json::Map::new();
    let format = args
        .get("format")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            Path::new(output_path)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_string)
        });
    if let Some(format) = format {
        body.insert("format".to_string(), json!(format));
    }

    let mut req = UniversalRequest::post("concat").with_body(Value::Object(body));
    for (i, entry) in inputs.iter().enumerate() {
        let path = entry.as_str().ok_or("input_paths must be strings")?;
        let field = if i == 0 { "file".to_string() } else { format!("file{}", i + 1) };
        req = req.with_attachment(local_attachment(&field, Path::new(path))?);
    }
    let result = run_local(req, Some(Path::new(output_path)))?;
    Ok(format!("Concatenated {} files -> {output_path} ({} bytes)", inputs.len(), result.size))
}

fn tool_raw(args: &Value) -> Result<String, String> {
    let input_path = str_arg(args, "input_path")?;
    let output_path = str_arg(args, "output_path")?;
    let raw_args = args.get("args").and_then(Value::as_array).ok_or("missing args array")?;

    let output_ext = Path::new(output_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mkv");
    let body = json!({ "args": raw_args, "output_ext": output_ext });

    let req = UniversalRequest::post("raw")
        .with_body(body)
        .with_attachment(local_attachment("file", Path::new(input_path))?);
    let result = run_local(req, Some(Path::new(output_path)))?;
    Ok(format!("Raw passthrough {input_path} -> {output_path} ({} bytes)", result.size))
}

fn tool_probe(args: &Value) -> Result<String, String> {
    let path = str_arg(args, "path")?;
    let req = UniversalRequest::post("probe")
        .with_attachment(local_attachment("file", Path::new(path))?);
    let result = run_local(req, None)?;
    serde_json::to_string_pretty(&result.body.unwrap_or(Value::Null)).map_err(|e| e.to_string())
}

fn tool_presets(_args: &Value) -> Result<String, String> {
    let result = run_local(UniversalRequest::get("presets"), None)?;
    serde_json::to_string_pretty(&result.body.unwrap_or(json!([]))).map_err(|e| e.to_string())
}

fn tool_capabilities(args: &Value) -> Result<String, String> {
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("all");
    let req = UniversalRequest::get("capabilities").with_query("kind", kind);
    let result = run_local(req, None)?;
    serde_json::to_string_pretty(&result.body.unwrap_or(json!({}))).map_err(|e| e.to_string())
}

fn json_rpc_result(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn json_rpc_error(id: Option<Value>, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}
