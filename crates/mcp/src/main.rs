//! media-convertor MCP server: stdio JSON-RPC with convert, probe, extract tools.
//!
//! Follows the file-processor-mcp pattern: custom stdio JSON-RPC, not the MCP SDK.

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
                    "description": "Convert a media file to another format",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input_path": { "type": "string", "description": "Path to input file" },
                            "output_path": { "type": "string", "description": "Path to output file" },
                            "preset": { "type": "string", "description": "Preset name (e.g. podcast-mp3, web-mp4)" },
                            "audio_codec": { "type": "string", "description": "Audio codec name" },
                            "video_codec": { "type": "string", "description": "Video codec name" },
                            "audio_bitrate": { "type": "integer", "description": "Audio bitrate in bps" },
                            "crf": { "type": "integer", "description": "Video CRF quality (lower = better)" }
                        },
                        "required": ["input_path", "output_path"]
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
                    "name": "extract_audio",
                    "description": "Extract audio track from a video file",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input_path": { "type": "string", "description": "Path to input video" },
                            "output_path": { "type": "string", "description": "Path to output audio file" }
                        },
                        "required": ["input_path", "output_path"]
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
        "probe" => tool_probe(&arguments),
        "extract_audio" => tool_extract_audio(&arguments),
        "presets" => tool_presets(),
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

use media_convertor_core::ffmpeg::command::{self, Trim};
use media_convertor_core::operation::ConvertRequest;
use media_convertor_core::progress::NoProgress;
use media_convertor_core::{Config, Container, Engine, Preset};
use std::path::Path;
use std::sync::OnceLock;

/// Lazily-built shared engine (locates ffmpeg + discovers capabilities once).
fn engine() -> Result<&'static Engine, String> {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    if let Some(e) = ENGINE.get() {
        return Ok(e);
    }
    let engine = Engine::new(&Config::default()).map_err(|e| e.to_string())?;
    Ok(ENGINE.get_or_init(|| engine))
}

fn tool_convert(args: &Value) -> Result<String, String> {
    let input_path = args.get("input_path").and_then(Value::as_str).ok_or("missing input_path")?;
    let output_path = args.get("output_path").and_then(Value::as_str).ok_or("missing output_path")?;
    let output = Path::new(output_path);

    let preset = args.get("preset").and_then(Value::as_str).map(str::to_string);
    let hint = Container::from_path(output);
    let cr = ConvertRequest {
        preset: preset.clone(),
        format: if preset.is_none() { hint.map(|c| c.extension().to_string()) } else { None },
        video_codec: args.get("video_codec").and_then(Value::as_str).map(str::to_string),
        audio_codec: args.get("audio_codec").and_then(Value::as_str).map(str::to_string),
        crf: args.get("crf").and_then(Value::as_u64).map(|v| v as u8),
        audio_bitrate: args.get("audio_bitrate").and_then(Value::as_u64).map(|v| v.to_string()),
        ..Default::default()
    };
    let fmt = cr.build_format(hint).map_err(|e| e.to_string())?;
    let cmd = command::transcode_args(Path::new(input_path), output, &fmt, &Trim::default());

    let total = engine()?.probe(Path::new(input_path)).ok().and_then(|i| i.duration);
    engine()?
        .run(&cmd, total, &mut NoProgress, None)
        .map_err(|e| e.to_string())?;
    let size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    Ok(format!("Converted {input_path} -> {output_path} ({size} bytes)"))
}

fn tool_probe(args: &Value) -> Result<String, String> {
    let path = args.get("path").and_then(Value::as_str).ok_or("missing path")?;
    let info = engine()?.probe(Path::new(path)).map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&info).map_err(|e| e.to_string())
}

fn tool_extract_audio(args: &Value) -> Result<String, String> {
    let input_path = args.get("input_path").and_then(Value::as_str).ok_or("missing input_path")?;
    let output_path = args.get("output_path").and_then(Value::as_str).ok_or("missing output_path")?;
    let codec = args.get("codec").and_then(Value::as_str);
    let cmd = command::extract_audio_args(Path::new(input_path), Path::new(output_path), codec);
    engine()?
        .run(&cmd, None, &mut NoProgress, None)
        .map_err(|e| e.to_string())?;
    Ok(format!("Extracted audio: {input_path} -> {output_path}"))
}

fn tool_presets() -> Result<String, String> {
    serde_json::to_string_pretty(Preset::all()).map_err(|e| e.to_string())
}

fn tool_capabilities(args: &Value) -> Result<String, String> {
    let caps = engine()?.capabilities();
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("all");
    let value = match kind {
        "encoders" => json!({ "encoders": caps.encoders }),
        "decoders" => json!({ "decoders": caps.decoders }),
        "filters" => json!({ "filters": caps.filters }),
        "muxers" => json!({ "muxers": caps.muxers }),
        "demuxers" => json!({ "demuxers": caps.demuxers }),
        _ => serde_json::to_value(caps).unwrap_or(json!({})),
    };
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
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
