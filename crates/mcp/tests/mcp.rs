//! MCP integration tests: spawn the compiled `media-convertor-mcp` binary and drive it
//! over stdio JSON-RPC, asserting on the advertised tool surface and on real conversions.
//!
//! ffmpeg-dependent tests self-skip when ffmpeg/ffprobe are not on PATH, matching the
//! CLI/server integration suites.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

/// Path to the MCP binary under test (set by Cargo for integration tests).
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_media-convertor-mcp")
}

fn ffmpeg_available() -> bool {
    let ok = Command::new("ffmpeg").arg("-version").output().is_ok()
        && Command::new("ffprobe").arg("-version").output().is_ok();
    if !ok {
        eprintln!("SKIP: ffmpeg/ffprobe not found");
    }
    ok
}

/// Create a short sine-wave WAV fixture.
fn make_wav(path: &Path, seconds: f64) {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .arg(path)
        .output()
        .expect("ffmpeg wav fixture");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// Create a short test video with both a video and an audio stream.
fn make_video(path: &Path, seconds: f64) {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc=duration={seconds}:size=160x120:rate=10"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .args(["-pix_fmt", "yuv420p", "-shortest"])
        .arg(path)
        .output()
        .expect("ffmpeg video fixture");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// Run a batch of JSON-RPC requests through a fresh MCP process and collect the responses.
/// The server reads one request per line and exits on stdin EOF.
fn session(work_dir: &Path, raw_enabled: bool, requests: &[Value]) -> Vec<Value> {
    let mut cmd = Command::new(bin());
    cmd.env("MEDIA_CONVERTOR_DATA", work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if raw_enabled {
        cmd.env("MEDIA_CONVERTOR_RAW", "1");
    } else {
        cmd.env_remove("MEDIA_CONVERTOR_RAW");
    }
    let mut child = cmd.spawn().expect("spawn media-convertor-mcp");

    let mut payload = String::new();
    for req in requests {
        payload.push_str(&req.to_string());
        payload.push('\n');
    }

    // Write on a separate thread while we drain stdout, so large responses can't deadlock.
    let mut stdin = child.stdin.take().expect("stdin");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(payload.as_bytes());
        // Dropping `stdin` here closes it, signalling EOF to the server.
    });

    let mut out = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut out)
        .expect("read stdout");
    let _ = writer.join();
    let _ = child.wait();

    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("bad json line {l:?}: {e}")))
        .collect()
}

/// A `tools/call` request for `tool` with the given arguments.
fn call(id: u64, tool: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments }
    })
}

/// Extract `(is_error, text)` from a `tools/call` response.
fn tool_result(resp: &Value) -> (bool, String) {
    let result = &resp["result"];
    let is_error = result["isError"].as_bool().unwrap_or(false);
    let text = result["content"][0]["text"].as_str().unwrap_or("").to_string();
    (is_error, text)
}

// ── Surface (no ffmpeg required) ────────────────────────────────

#[test]
fn tools_list_is_full_non_admin_surface() {
    let dir = tempfile::tempdir().unwrap();
    let resp = session(
        dir.path(),
        false,
        &[json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})],
    );
    let tools = resp[0]["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for expected in [
        "convert",
        "extract_audio",
        "thumbnail",
        "filter",
        "concat",
        "raw",
        "probe",
        "presets",
        "capabilities",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}: {names:?}");
    }
    // Admin/lifecycle controls must never be exposed.
    for forbidden in ["server", "start", "stop", "status"] {
        assert!(!names.contains(&forbidden), "admin tool leaked: {forbidden}");
    }

    // `extract_audio` advertises its optional codec argument.
    let extract = tools.iter().find(|t| t["name"] == "extract_audio").unwrap();
    assert!(extract["inputSchema"]["properties"]["codec"].is_object());
}

#[test]
fn initialize_reports_server_info() {
    let dir = tempfile::tempdir().unwrap();
    let resp = session(
        dir.path(),
        false,
        &[json!({"jsonrpc":"2.0","id":1,"method":"initialize"})],
    );
    assert_eq!(resp[0]["result"]["serverInfo"]["name"], "media-convertor-mcp");
    assert!(resp[0]["result"]["protocolVersion"].is_string());
}

#[test]
fn unknown_tool_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let resp = session(dir.path(), false, &[call(1, "nonexistent", json!({}))]);
    let (is_error, text) = tool_result(&resp[0]);
    assert!(is_error);
    assert!(text.contains("unknown tool"), "text: {text}");
}

// ── Real conversions (ffmpeg required) ──────────────────────────

#[test]
fn convert_then_probe() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("in.wav");
    let mp3 = dir.path().join("out.mp3");
    make_wav(&wav, 1.0);

    let resp = session(
        dir.path(),
        false,
        &[
            call(1, "convert", json!({
                "input_path": wav.to_str().unwrap(),
                "output_path": mp3.to_str().unwrap(),
                "audio_bitrate": "128k"
            })),
            call(2, "probe", json!({ "path": mp3.to_str().unwrap() })),
        ],
    );

    let (err1, _) = tool_result(&resp[0]);
    assert!(!err1, "convert failed: {:?}", tool_result(&resp[0]));
    assert!(mp3.exists(), "output not written");

    let (err2, probe_text) = tool_result(&resp[1]);
    assert!(!err2, "probe failed: {probe_text}");
    assert!(probe_text.contains("mp3"), "probe json: {probe_text}");
}

#[test]
fn extract_audio_from_video() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("clip.mp4");
    let audio = dir.path().join("track.m4a");
    make_video(&video, 1.0);

    let resp = session(
        dir.path(),
        false,
        &[call(1, "extract_audio", json!({
            "input_path": video.to_str().unwrap(),
            "output_path": audio.to_str().unwrap(),
            "codec": "aac"
        }))],
    );
    let (err, text) = tool_result(&resp[0]);
    assert!(!err, "extract_audio failed: {text}");
    assert!(audio.exists());
}

#[test]
fn thumbnail_filter_and_concat_produce_outputs() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.mp4");
    let b = dir.path().join("b.mp4");
    make_video(&a, 1.0);
    make_video(&b, 1.0);
    let thumb = dir.path().join("thumb.jpg");
    let filtered = dir.path().join("filtered.mp4");
    let joined = dir.path().join("joined.mkv");

    let resp = session(
        dir.path(),
        false,
        &[
            call(1, "thumbnail", json!({
                "input_path": a.to_str().unwrap(),
                "output_path": thumb.to_str().unwrap(),
                "time": 0.5,
                "width": 80
            })),
            call(2, "filter", json!({
                "input_path": a.to_str().unwrap(),
                "output_path": filtered.to_str().unwrap(),
                "graph": "scale=80:-2"
            })),
            call(3, "concat", json!({
                "input_paths": [a.to_str().unwrap(), b.to_str().unwrap()],
                "output_path": joined.to_str().unwrap()
            })),
        ],
    );

    for (i, name) in ["thumbnail", "filter", "concat"].iter().enumerate() {
        let (err, text) = tool_result(&resp[i]);
        assert!(!err, "{name} failed: {text}");
    }
    assert!(thumb.exists() && filtered.exists() && joined.exists());
}

#[test]
fn concat_rejects_single_input() {
    let dir = tempfile::tempdir().unwrap();
    let resp = session(
        dir.path(),
        false,
        &[call(1, "concat", json!({
            "input_paths": ["only-one.mp4"],
            "output_path": "out.mkv"
        }))],
    );
    let (err, text) = tool_result(&resp[0]);
    assert!(err);
    assert!(text.contains("at least 2"), "text: {text}");
}

#[test]
fn raw_is_gated_off_by_default_and_on_with_env() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.mp4");
    make_video(&input, 1.0);
    let off_out = dir.path().join("off.mp4");
    let on_out = dir.path().join("on.mp4");

    let raw_call = |id: u64, out: &Path| {
        call(id, "raw", json!({
            "input_path": input.to_str().unwrap(),
            "output_path": out.to_str().unwrap(),
            "args": ["-i", "INPUT", "-vf", "hue=s=0", "OUTPUT"]
        }))
    };

    // Disabled by default: security error, no output.
    let off = session(dir.path(), false, &[raw_call(1, &off_out)]);
    let (err_off, text_off) = tool_result(&off[0]);
    assert!(err_off, "raw should be gated off");
    assert!(text_off.contains("disabled"), "text: {text_off}");
    assert!(!off_out.exists(), "gated-off raw must not write output");

    // Enabled via MEDIA_CONVERTOR_RAW: succeeds.
    let on = session(dir.path(), true, &[raw_call(2, &on_out)]);
    let (err_on, text_on) = tool_result(&on[0]);
    assert!(!err_on, "raw with env enabled failed: {text_on}");
    assert!(on_out.exists());
}

#[test]
fn presets_and_capabilities_are_inline() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let resp = session(
        dir.path(),
        false,
        &[
            call(1, "presets", json!({})),
            call(2, "capabilities", json!({ "kind": "muxers" })),
        ],
    );

    let (err1, presets) = tool_result(&resp[0]);
    assert!(!err1);
    assert!(presets.contains("podcast-mp3"), "presets: {presets}");

    let (err2, caps) = tool_result(&resp[1]);
    assert!(!err2);
    assert!(caps.contains("muxers"), "caps: {caps}");
}
