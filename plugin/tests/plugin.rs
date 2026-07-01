//! Plugin integration tests: spawn the compiled `media-convertor-plugin` binary, feed
//! audio bytes on stdin, and assert on the converted bytes written to stdout.
//!
//! ffmpeg-dependent tests self-skip when ffmpeg/ffprobe are not on PATH; the empty-input
//! test runs unconditionally because the plugin rejects it before touching ffmpeg.

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_media-convertor-plugin")
}

fn ffmpeg_available() -> bool {
    let ok = Command::new("ffmpeg").arg("-version").output().is_ok()
        && Command::new("ffprobe").arg("-version").output().is_ok();
    if !ok {
        eprintln!("SKIP: ffmpeg/ffprobe not found");
    }
    ok
}

/// Generate raw WAV bytes to feed the plugin.
fn wav_bytes(seconds: f64) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("in.wav");
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .arg(&path)
        .output()
        .expect("ffmpeg wav fixture");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::read(&path).expect("read wav fixture")
}

/// Result of a plugin run: exit success, stdout bytes, stderr text.
struct PluginOut {
    success: bool,
    stdout: Vec<u8>,
    stderr: String,
}

/// Run the plugin with `input` on stdin and the given `PLUGIN_OPT_*` env vars.
fn run_plugin(input: &[u8], env: &[(&str, &str)]) -> PluginOut {
    let mut cmd = Command::new(bin());
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn media-convertor-plugin");

    // Feed stdin on a thread so writing can't deadlock against stdout being filled.
    let mut stdin = child.stdin.take().expect("stdin");
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
        // `stdin` drops here, closing the pipe (EOF).
    });

    let output = child.wait_with_output().expect("wait plugin");
    let _ = writer.join();
    PluginOut {
        success: output.status.success(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

/// Probe the codec of some produced bytes by writing them out and running ffprobe.
fn probe_codec(bytes: &[u8], ext: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("out.{ext}"));
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new("ffprobe")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(&path)
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn empty_input_is_rejected() {
    let result = run_plugin(&[], &[]);
    assert!(!result.success, "empty input should fail");
    assert!(result.stderr.contains("no input"), "stderr: {}", result.stderr);
}

#[test]
fn converts_wav_to_mp3_by_default() {
    if !ffmpeg_available() {
        return;
    }
    let input = wav_bytes(1.0);
    let result = run_plugin(&input, &[]);
    assert!(result.success, "plugin failed: {}", result.stderr);
    assert!(!result.stdout.is_empty(), "no output produced");
    assert_eq!(probe_codec(&result.stdout, "mp3"), "mp3");
}

#[test]
fn honors_codec_option() {
    if !ffmpeg_available() {
        return;
    }
    let input = wav_bytes(1.0);
    let result = run_plugin(&input, &[("PLUGIN_OPT_CODEC", "flac")]);
    assert!(result.success, "plugin failed: {}", result.stderr);
    assert_eq!(probe_codec(&result.stdout, "flac"), "flac");
}

#[test]
fn honors_preset_option() {
    if !ffmpeg_available() {
        return;
    }
    let input = wav_bytes(1.0);
    let result = run_plugin(&input, &[("PLUGIN_OPT_PRESET", "podcast-mp3")]);
    assert!(result.success, "plugin failed: {}", result.stderr);
    assert_eq!(probe_codec(&result.stdout, "mp3"), "mp3");
}

#[test]
fn unknown_codec_falls_back_to_mp3() {
    if !ffmpeg_available() {
        return;
    }
    let input = wav_bytes(1.0);
    let result = run_plugin(&input, &[("PLUGIN_OPT_CODEC", "not-a-codec")]);
    assert!(result.success, "plugin failed: {}", result.stderr);
    assert!(result.stderr.contains("unknown codec"), "stderr: {}", result.stderr);
    assert_eq!(probe_codec(&result.stdout, "mp3"), "mp3");
}

#[test]
fn honors_bitrate_and_sample_rate_options() {
    if !ffmpeg_available() {
        return;
    }
    let input = wav_bytes(1.0);
    let result = run_plugin(
        &input,
        &[
            ("PLUGIN_OPT_CODEC", "mp3"),
            ("PLUGIN_OPT_BITRATE", "96k"),
            ("PLUGIN_OPT_SAMPLE_RATE", "22050"),
        ],
    );
    assert!(result.success, "plugin failed: {}", result.stderr);
    assert_eq!(probe_codec(&result.stdout, "mp3"), "mp3");
}
