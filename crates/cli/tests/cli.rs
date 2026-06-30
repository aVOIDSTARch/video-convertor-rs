//! CLI integration tests: invoke the compiled `media-convertor` binary as a subprocess
//! and assert on exit codes, output files, and stdout.

use std::path::Path;
use std::process::{Command, Output};

/// Path to the binary under test (set by Cargo for integration tests).
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_media-convertor")
}

fn ffmpeg_available() -> bool {
    let ok = Command::new("ffmpeg").arg("-version").output().is_ok()
        && Command::new("ffprobe").arg("-version").output().is_ok();
    if !ok {
        eprintln!("SKIP: ffmpeg/ffprobe not found");
    }
    ok
}

fn make_wav(path: &Path, seconds: f64) {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .arg(path)
        .output()
        .expect("ffmpeg fixture");
    assert!(out.status.success());
}

fn run(args: &[&str], work_dir: &Path) -> Output {
    Command::new(bin())
        .args(["--work-dir", work_dir.to_str().unwrap()])
        .args(args)
        .output()
        .expect("spawn cli")
}

#[test]
fn version_and_help_need_no_ffmpeg() {
    let v = Command::new(bin()).arg("--version").output().unwrap();
    assert!(v.status.success());
    assert!(String::from_utf8_lossy(&v.stdout).contains("media-convertor"));

    let h = Command::new(bin()).arg("--help").output().unwrap();
    assert!(h.status.success());
    let text = String::from_utf8_lossy(&h.stdout);
    assert!(text.contains("convert"));
    assert!(text.contains("server"));
}

#[test]
fn convert_then_probe() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("in.wav");
    let mp3 = dir.path().join("out.mp3");
    make_wav(&wav, 2.0);

    let out = run(
        &[
            "convert",
            wav.to_str().unwrap(),
            "-o",
            mp3.to_str().unwrap(),
            "--preset",
            "podcast-mp3",
            "-q",
        ],
        dir.path(),
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(mp3.exists());

    let probe = run(&["probe", mp3.to_str().unwrap()], dir.path());
    assert!(probe.status.success());
    let json = String::from_utf8_lossy(&probe.stdout);
    assert!(json.contains("\"mp3\""), "probe json: {json}");
}

#[test]
fn presets_and_capabilities() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();

    let presets = run(&["presets"], dir.path());
    assert!(presets.status.success());
    assert!(String::from_utf8_lossy(&presets.stdout).contains("podcast-mp3"));

    let caps = run(&["capabilities", "encoders"], dir.path());
    assert!(caps.status.success());
    assert!(String::from_utf8_lossy(&caps.stdout).contains("\"name\""));
}

#[test]
fn explicit_flags_convert() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("in.wav");
    let flac = dir.path().join("out.flac");
    make_wav(&wav, 1.0);

    let out = run(
        &[
            "convert",
            wav.to_str().unwrap(),
            "-o",
            flac.to_str().unwrap(),
            "--audio-codec",
            "flac",
            "-q",
        ],
        dir.path(),
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(flac.exists());
}

#[test]
fn batch_manifest() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("in.wav");
    make_wav(&wav, 1.0);
    let out1 = dir.path().join("a.mp3");
    let out2 = dir.path().join("b.flac");
    let manifest = dir.path().join("jobs.txt");
    std::fs::write(
        &manifest,
        format!(
            "# comment\n{} {} podcast-mp3\n{} {}\n",
            wav.display(),
            out1.display(),
            wav.display(),
            out2.display()
        ),
    )
    .unwrap();

    let out = run(&["batch", manifest.to_str().unwrap()], dir.path());
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out1.exists());
    assert!(out2.exists());
}

#[test]
fn missing_input_errors() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        &[
            "convert",
            "/no/such/input.wav",
            "-o",
            dir.path().join("x.mp3").to_str().unwrap(),
            "--preset",
            "podcast-mp3",
            "-q",
        ],
        dir.path(),
    );
    assert!(!out.status.success(), "should fail on missing input");
}

#[test]
fn raw_requires_enable_flag() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("in.wav");
    make_wav(&wav, 1.0);
    let out = run(
        &[
            "raw",
            wav.to_str().unwrap(),
            "-o",
            dir.path().join("o.mp3").to_str().unwrap(),
            "--",
            "-c:a",
            "libmp3lame",
        ],
        dir.path(),
    );
    assert!(!out.status.success(), "raw without --enable-raw should fail");
    assert!(String::from_utf8_lossy(&out.stderr).contains("enable-raw"));
}

#[test]
fn server_status_when_not_running() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(&["server", "status"], dir.path());
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("not running"));
}

#[test]
fn server_start_rejects_remote_bind_without_token() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        &["server", "start", "--host", "0.0.0.0", "--foreground"],
        dir.path(),
    );
    assert!(!out.status.success(), "remote bind without token must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("token") || stderr.contains("security"), "{stderr}");
}
