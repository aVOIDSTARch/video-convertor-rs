//! media-convertor-core: Rust media conversion library powered by FFmpeg.
//!
//! Provides codec/container enums, format specification types, a preset system,
//! and (when FFmpeg features are enabled) probing, transcoding, and streaming I/O.

// ── Phase 1: data types (no ffmpeg dependency) ─────────────────
pub mod codec;
pub mod config;
pub mod container;
pub mod error;
pub mod format;
pub mod preset;
pub mod progress;

// ── Phase 2: transcode, probe, filters, I/O ────────────────────
pub mod audio;
pub mod detect;
pub mod filter;
pub mod stream;
pub mod transcode;
pub mod video;

// Re-exports for convenience.
pub use codec::{AudioCodec, VideoCodec};
pub use config::AppConfig;
pub use container::Container;
pub use detect::{MediaInfo, StreamInfo, StreamType};
pub use error::{MediaError, Result};
pub use filter::{AudioFilterChain, VideoFilterChain};
pub use format::{AudioSettings, MediaFormat, VideoSettings};
pub use preset::{Preset, PresetCategory};
pub use progress::{NoProgress, ProgressEvent, ProgressHandler};
pub use stream::{InputSource, OutputTarget};
pub use transcode::{CancelToken, TranscodeJob, TranscodeResult};

use std::sync::Once;

static FFMPEG_INIT: Once = Once::new();

/// Initialise FFmpeg (safe to call multiple times).
pub fn init() {
    FFMPEG_INIT.call_once(|| {
        #[cfg(any(feature = "bundled", feature = "system-ffmpeg"))]
        ffmpeg_next::init().expect("failed to initialise ffmpeg");
    });
}
