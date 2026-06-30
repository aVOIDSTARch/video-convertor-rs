//! Shared helpers for integration tests: ffmpeg availability guard and fixture
//! generation. Tests that need ffmpeg call [`require_ffmpeg`] and return early (skipping)
//! when it is unavailable, so the suite still passes on machines without ffmpeg.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns true if `ffmpeg` and `ffprobe` are runnable. Prints a skip notice otherwise.
pub fn require_ffmpeg() -> bool {
    let ok = Command::new("ffmpeg").arg("-version").output().is_ok()
        && Command::new("ffprobe").arg("-version").output().is_ok();
    if !ok {
        eprintln!("SKIP: ffmpeg/ffprobe not found on PATH");
    }
    ok
}

/// Generate a short test WAV (sine tone) at `path`.
pub fn make_wav(path: &Path, seconds: f64) {
    run_ffmpeg(&[
        "-y",
        "-f",
        "lavfi",
        "-i",
        &format!("sine=frequency=440:duration={seconds}"),
        path.to_str().unwrap(),
    ]);
}

/// Generate a short test MP4 (testsrc video + sine audio) at `path`.
pub fn make_mp4(path: &Path, seconds: f64) {
    run_ffmpeg(&[
        "-y",
        "-f",
        "lavfi",
        "-i",
        &format!("testsrc=duration={seconds}:size=320x240:rate=15"),
        "-f",
        "lavfi",
        "-i",
        &format!("sine=duration={seconds}"),
        "-shortest",
        "-pix_fmt",
        "yuv420p",
        path.to_str().unwrap(),
    ]);
}

/// Run the system ffmpeg directly (used for fixture generation, not via the engine).
fn run_ffmpeg(args: &[&str]) {
    let out = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .args(args)
        .output()
        .expect("failed to spawn ffmpeg for fixture");
    assert!(
        out.status.success(),
        "fixture ffmpeg failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Probe a file's first stream codec names via ffprobe (for assertions).
pub fn stream_codecs(path: &Path) -> Vec<String> {
    let out = Command::new("ffprobe")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// A scratch directory that cleans itself up.
pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// Build a [`media_convertor_core::Config`] rooted at a temp work dir.
pub fn test_config(dir: &Path) -> media_convertor_core::Config {
    let mut c = media_convertor_core::Config::default();
    c.work_dir = dir.to_path_buf();
    c.job_timeout_secs = 120;
    c
}

/// Convenience: a path inside `dir`.
pub fn child(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}
