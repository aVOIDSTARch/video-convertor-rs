//! Subprocess FFmpeg engine.
//!
//! Locates the `ffmpeg`/`ffprobe` binaries, discovers their full capability set, runs
//! jobs from *argument vectors* (never shell strings) with progress reporting, a
//! wall-clock timeout, and cooperative cancellation, and probes media via `ffprobe`.

pub mod capabilities;
pub mod command;
pub mod locate;
pub mod probe;
pub mod run;

pub use capabilities::{Capabilities, CodecEntry, FilterEntry, MediaKind};
pub use locate::Tools;

use crate::config::Config;
use crate::detect::MediaInfo;
use crate::progress::ProgressHandler;
use crate::Result;
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A cancellation token shared across threads. Cancelling kills the running ffmpeg child.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Signal cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// The high-level FFmpeg engine: resolved tools + discovered capabilities + run policy.
#[derive(Clone)]
pub struct Engine {
    tools: Tools,
    caps: Arc<Capabilities>,
    threads: u32,
    job_timeout: Option<Duration>,
}

impl Engine {
    /// Build an engine from configuration: locate the binaries and discover capabilities.
    pub fn new(config: &Config) -> Result<Self> {
        let tools = locate::locate(config.ffmpeg_path.as_deref(), config.ffprobe_path.as_deref())?;
        let caps = capabilities::discover(&tools)?;
        Ok(Self {
            tools,
            caps: Arc::new(caps),
            threads: config.threads,
            job_timeout: config.job_timeout(),
        })
    }

    /// The resolved binary paths and version string.
    pub fn tools(&self) -> &Tools {
        &self.tools
    }

    /// The discovered capability set (encoders, decoders, filters, …).
    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// Probe a media file into structured [`MediaInfo`] via `ffprobe`.
    pub fn probe(&self, path: &std::path::Path) -> Result<MediaInfo> {
        probe::probe(&self.tools.ffprobe, path)
    }

    /// Run an ffmpeg job from an already-built (and validated) argument vector.
    ///
    /// Global safety flags (`-hide_banner -nostdin -y`, protocol whitelist, threads) are
    /// prepended here; callers supply only the operation-specific args (`-i in … out`).
    pub fn run(
        &self,
        op_args: &[OsString],
        total: Option<Duration>,
        progress: &mut dyn ProgressHandler,
        cancel: Option<&CancelToken>,
    ) -> Result<()> {
        let mut args: Vec<OsString> = vec![
            "-hide_banner".into(),
            "-nostdin".into(),
            "-y".into(),
            "-loglevel".into(),
            "error".into(),
            // Confine demuxer-level protocols to local files.
            "-protocol_whitelist".into(),
            crate::security::PROTOCOL_WHITELIST.into(),
            // Machine-readable progress on stdout.
            "-progress".into(),
            "pipe:1".into(),
            "-nostats".into(),
        ];
        if self.threads > 0 {
            args.push("-threads".into());
            args.push(self.threads.to_string().into());
        }
        args.extend_from_slice(op_args);

        run::run(
            &self.tools.ffmpeg,
            &args,
            total,
            progress,
            cancel,
            self.job_timeout,
        )
    }
}
