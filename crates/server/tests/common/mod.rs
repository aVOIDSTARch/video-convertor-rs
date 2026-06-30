//! Shared helpers for server integration tests: ffmpeg guard, fixtures, and a real
//! server bound to an ephemeral port.

#![allow(dead_code)]

use media_convertor_core::Config;
use media_convertor_server::{build_app, build_state};
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;

pub fn require_ffmpeg() -> bool {
    let ok = Command::new("ffmpeg").arg("-version").output().is_ok()
        && Command::new("ffprobe").arg("-version").output().is_ok();
    if !ok {
        eprintln!("SKIP: ffmpeg/ffprobe not found on PATH");
    }
    ok
}

pub fn make_wav(path: &Path, seconds: f64) {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .arg(path)
        .output()
        .expect("ffmpeg fixture");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// A running test server. Holds the work dir so it lives for the test's lifetime.
pub struct TestServer {
    pub base: String,
    pub _dir: tempfile::TempDir,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    /// Start a server on 127.0.0.1:0 with an optional token, returning its base URL.
    pub async fn start(token: Option<&str>) -> Self {
        let dir = tempdir();
        let mut config = Config::default();
        config.work_dir = dir.path().to_path_buf();
        config.workers = 2;
        config.job_timeout_secs = 120;
        config.raw_enabled = false;
        config.token = token.map(str::to_string);

        let state = build_state(config).expect("build state");
        let app = build_app(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            base: format!("http://{addr}/api/v1"),
            _dir: dir,
            handle,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path.trim_start_matches('/'))
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
