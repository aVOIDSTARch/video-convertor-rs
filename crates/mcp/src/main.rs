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
                    "name": "formats",
                    "description": "List supported formats and codecs",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
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
        "formats" => tool_formats(),
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

fn tool_convert(args: &Value) -> Result<String, String> {
    let input_path = args
        .get("input_path")
        .and_then(|v| v.as_str())
        .ok_or("missing input_path")?;
    let output_path = args
        .get("output_path")
        .and_then(|v| v.as_str())
        .ok_or("missing output_path")?;
    let preset_name = args.get("preset").and_then(|v| v.as_str());

    let format = if let Some(name) = preset_name {
        let preset = media_convertor_core::preset::Preset::by_name(name)
            .ok_or_else(|| format!("unknown preset: {name}"))?;
        preset.format.clone()
    } else {
        // Infer from output extension
        let container = media_convertor_core::container::Container::from_path(
            std::path::Path::new(output_path),
        )
        .ok_or("cannot determine format from output extension")?;
        media_convertor_core::format::MediaFormat::remux(container)
    };

    #[cfg(feature = "ffmpeg")]
    {
        let job = media_convertor_core::transcode::TranscodeJob::new(
            std::path::PathBuf::from(input_path),
            std::path::PathBuf::from(output_path),
            format,
        );
        let result = media_convertor_core::transcode::transcode_simple(job)
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "Converted {} -> {} ({} bytes)",
            input_path, output_path, result.output_size
        ))
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        let _ = (input_path, output_path, format);
        Err("FFmpeg not available in this build".to_string())
    }
}

fn tool_probe(args: &Value) -> Result<String, String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing path")?;

    #[cfg(feature = "ffmpeg")]
    {
        let info = media_convertor_core::detect::probe_file(std::path::Path::new(path))
            .map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&info).map_err(|e| e.to_string())
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        let _ = path;
        Err("FFmpeg not available in this build".to_string())
    }
}

fn tool_extract_audio(args: &Value) -> Result<String, String> {
    let input_path = args
        .get("input_path")
        .and_then(|v| v.as_str())
        .ok_or("missing input_path")?;
    let output_path = args
        .get("output_path")
        .and_then(|v| v.as_str())
        .ok_or("missing output_path")?;

    #[cfg(feature = "ffmpeg")]
    {
        media_convertor_core::audio::extract_audio(
            std::path::Path::new(input_path),
            std::path::Path::new(output_path),
        )
        .map_err(|e| e.to_string())?;
        Ok(format!("Extracted audio: {} -> {}", input_path, output_path))
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        let _ = (input_path, output_path);
        Err("FFmpeg not available in this build".to_string())
    }
}

fn tool_presets() -> Result<String, String> {
    let presets = media_convertor_core::preset::Preset::all();
    serde_json::to_string_pretty(presets).map_err(|e| e.to_string())
}

fn tool_formats() -> Result<String, String> {
    let containers: Vec<_> = media_convertor_core::container::Container::all()
        .iter()
        .map(|c| {
            json!({
                "name": format!("{:?}", c),
                "extension": c.extension(),
                "supports_video": c.supports_video(),
                "supports_audio": c.supports_audio(),
            })
        })
        .collect();

    let audio_codecs: Vec<_> = media_convertor_core::codec::AudioCodec::all()
        .iter()
        .map(|c| json!({ "name": format!("{:?}", c), "display_name": c.display_name() }))
        .collect();

    let video_codecs: Vec<_> = media_convertor_core::codec::VideoCodec::all()
        .iter()
        .map(|c| json!({ "name": format!("{:?}", c), "display_name": c.display_name() }))
        .collect();

    let output = json!({
        "containers": containers,
        "audio_codecs": audio_codecs,
        "video_codecs": video_codecs,
    });

    serde_json::to_string_pretty(&output).map_err(|e| e.to_string())
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
