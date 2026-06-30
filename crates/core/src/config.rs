//! Application-level configuration shared by the library, CLI, and server.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default HTTP port for the API server.
pub const DEFAULT_PORT: u16 = 3400;

/// Top-level configuration controlling the engine, work directories, the queue,
/// resource limits, and the (optional) HTTP server surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Explicit path to the `ffmpeg` binary. `None` = resolve `ffmpeg` on `PATH`.
    pub ffmpeg_path: Option<PathBuf>,
    /// Explicit path to the `ffprobe` binary. `None` = resolve `ffprobe` on `PATH`.
    pub ffprobe_path: Option<PathBuf>,

    /// Base working directory. Holds `uploads/`, `output/`, and `jobs/` sub-dirs.
    pub work_dir: PathBuf,

    /// Number of concurrent worker threads draining the queue.
    pub workers: usize,
    /// Per-job wall-clock timeout in seconds. 0 = no timeout.
    pub job_timeout_secs: u64,
    /// FFmpeg threads per job (passed as `-threads`). 0 = ffmpeg decides.
    pub threads: u32,

    /// Maximum accepted upload size in bytes (server multipart).
    pub max_upload_bytes: u64,
    /// Maximum produced output size in bytes (0 = unlimited).
    pub max_output_bytes: u64,

    /// Whether the gated raw-passthrough operation is permitted.
    pub raw_enabled: bool,

    /// Host the server binds to. Binding anything other than loopback requires `token`.
    pub host: String,
    /// Port the server binds to.
    pub port: u16,
    /// Bearer token required for API requests. Required to bind a non-loopback host.
    pub token: Option<String>,
}

/// Default base work directory: `~/.media-convertor` when a home directory is known,
/// otherwise `$TMPDIR/media-convertor`. Chosen to be stable across reboots so a
/// background server's queue, uploads, outputs, and pidfile persist.
fn default_work_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".media-convertor");
        }
    }
    std::env::temp_dir().join("media-convertor")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ffmpeg_path: None,
            ffprobe_path: None,
            work_dir: default_work_dir(),
            workers: 2,
            job_timeout_secs: 3600,
            threads: 0,
            max_upload_bytes: 2 * 1024 * 1024 * 1024, // 2 GiB
            max_output_bytes: 0,
            raw_enabled: false,
            host: "127.0.0.1".to_string(),
            port: DEFAULT_PORT,
            token: None,
        }
    }
}

impl Config {
    /// Directory holding uploaded input files.
    pub fn upload_dir(&self) -> PathBuf {
        self.work_dir.join("uploads")
    }

    /// Directory holding produced output files.
    pub fn output_dir(&self) -> PathBuf {
        self.work_dir.join("output")
    }

    /// Directory holding persisted job records (`{id}.json`).
    pub fn jobs_dir(&self) -> PathBuf {
        self.work_dir.join("jobs")
    }

    /// Path to the server pidfile (used by the CLI to manage a background server).
    pub fn pid_file(&self) -> PathBuf {
        self.work_dir.join("server.pid")
    }

    /// Path to the server logfile (used when started in the background).
    pub fn log_file(&self) -> PathBuf {
        self.work_dir.join("server.log")
    }

    /// Create all managed work sub-directories.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for d in [self.upload_dir(), self.output_dir(), self.jobs_dir()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    /// Whether the configured bind host is a loopback address.
    pub fn is_loopback(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "::1" | "localhost")
    }

    /// Job timeout as a `Duration`, or `None` when disabled.
    pub fn job_timeout(&self) -> Option<std::time::Duration> {
        if self.job_timeout_secs == 0 {
            None
        } else {
            Some(std::time::Duration::from_secs(self.job_timeout_secs))
        }
    }

    /// Validate the security posture of the server configuration.
    ///
    /// Refuses to expose a non-loopback host without a bearer token.
    pub fn validate_server(&self) -> crate::Result<()> {
        if !self.is_loopback() && self.host != "0.0.0.0" && self.token.is_none() {
            // Any explicit non-loopback bind needs a token.
        }
        if !self.is_loopback() && self.token.is_none() {
            return Err(crate::MediaError::security(format!(
                "refusing to bind non-loopback host '{}' without a bearer token; \
                 set a token or bind 127.0.0.1",
                self.host
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dirs_are_under_work_dir() {
        let c = Config::default();
        assert!(c.upload_dir().starts_with(&c.work_dir));
        assert!(c.output_dir().starts_with(&c.work_dir));
        assert!(c.jobs_dir().starts_with(&c.work_dir));
    }

    #[test]
    fn default_work_dir_is_stable() {
        let c = Config::default();
        // Ends in `.media-convertor` (under $HOME) or `media-convertor` (temp fallback).
        let name = c.work_dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert!(
            name == ".media-convertor" || name == "media-convertor",
            "unexpected default work dir: {}",
            c.work_dir.display()
        );
    }

    #[test]
    fn loopback_detection() {
        let mut c = Config::default();
        assert!(c.is_loopback());
        c.host = "0.0.0.0".to_string();
        assert!(!c.is_loopback());
    }

    #[test]
    fn non_loopback_without_token_is_rejected() {
        let mut c = Config::default();
        c.host = "192.168.1.10".to_string();
        assert!(c.validate_server().is_err());
        c.token = Some("secret".to_string());
        assert!(c.validate_server().is_ok());
    }

    #[test]
    fn loopback_needs_no_token() {
        let c = Config::default();
        assert!(c.validate_server().is_ok());
    }

    #[test]
    fn timeout_zero_is_none() {
        let mut c = Config::default();
        c.job_timeout_secs = 0;
        assert!(c.job_timeout().is_none());
        c.job_timeout_secs = 5;
        assert_eq!(c.job_timeout(), Some(std::time::Duration::from_secs(5)));
    }
}
