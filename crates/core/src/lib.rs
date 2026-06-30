//! media-convertor-core: a comprehensive, secure Rust shell over the FFmpeg/ffprobe
//! binaries.
//!
//! This crate is the single library that backs both the CLI (control plane) and the
//! HTTP API, and is designed to be imported directly by other projects.
//!
//! The design has three layers:
//!
//! 1. [`ffmpeg`] — a subprocess engine that locates the `ffmpeg`/`ffprobe` binaries,
//!    discovers their full capability set at runtime, builds *safe* argument vectors
//!    (never shell strings), and runs jobs with progress, timeouts, and cancellation.
//! 2. [`operation`] / [`dispatch`] — structured, validated operations (convert, probe,
//!    thumbnail, filter, concat, …) plus a gated raw passthrough, addressed through a
//!    universal HTTP-shaped request.
//! 3. [`api_queue`] — a self-contained, reusable, persistent job queue whose work items
//!    are universal HTTP-shaped request objects. It is generic over a [`api_queue::QueueHandler`];
//!    FFmpeg is just one handler implementation ([`dispatch::FfmpegHandler`]).
//!
//! Everything (CLI and API alike) flows: build a [`api_queue::UniversalRequest`] →
//! enqueue → a worker dispatches it to the engine.

// ── Pure data types (codecs, containers, formats, presets) ─────
pub mod codec;
pub mod config;
pub mod container;
pub mod detect;
pub mod error;
pub mod format;
pub mod preset;
pub mod progress;

// ── Security primitives (path confinement, sanitization, allowlists) ──
pub mod security;

// ── Subprocess FFmpeg engine ───────────────────────────────────
pub mod ffmpeg;

// ── Operations, routing, dispatch ──────────────────────────────
pub mod operation;
pub mod dispatch;

// ── Reusable persistent request queue ──────────────────────────
pub mod api_queue;

// Re-exports for convenience.
pub use codec::{AudioCodec, VideoCodec};
pub use config::Config;
pub use container::Container;
pub use detect::{MediaInfo, StreamInfo, StreamType};
pub use error::{MediaError, Result};
pub use ffmpeg::{Capabilities, CancelToken, Engine, Tools};
pub use format::{AudioSettings, MediaFormat, VideoSettings};
pub use operation::{ConvertRequest, Operation};
pub use preset::{Preset, PresetCategory};
pub use progress::{NoProgress, ProgressEvent, ProgressHandler};
pub use api_queue::{
    Attachment, Job, JobResult, JobStatus, Method, Queue, QueueHandler, UniversalRequest,
};
pub use dispatch::FfmpegHandler;
