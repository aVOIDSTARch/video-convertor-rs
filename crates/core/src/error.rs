//! Error types for media conversion operations.

use crate::codec::{AudioCodec, VideoCodec};
use crate::container::Container;

/// Top-level error type for media conversion operations.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("probe failed: {0}")]
    Probe(#[from] ProbeError),
    #[error("transcode error: {0}")]
    Transcode(#[from] TranscodeError),
    #[error("codec error: {0}")]
    Codec(#[from] CodecError),
    #[error("preset error: {0}")]
    Preset(#[from] PresetError),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    /// The ffmpeg/ffprobe binary could not be located.
    #[error("ffmpeg tool not found: {0}")]
    ToolNotFound(String),
    /// A security policy was violated (path escape, bad protocol, disallowed raw arg).
    #[error("security violation: {0}")]
    Security(String),
    /// A job exceeded its allotted wall-clock time and was killed.
    #[error("operation timed out after {0:?}")]
    Timeout(std::time::Duration),
    /// The operation was cancelled.
    #[error("operation cancelled")]
    Cancelled,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ffmpeg error: {0}")]
    Ffmpeg(String),
    #[error("{0}")]
    Other(String),
}

impl MediaError {
    /// Convenience constructor for ad-hoc messages.
    pub fn other(msg: impl Into<String>) -> Self {
        MediaError::Other(msg.into())
    }

    /// Convenience constructor for security violations.
    pub fn security(msg: impl Into<String>) -> Self {
        MediaError::Security(msg.into())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("cannot open file: {0}")]
    OpenFailed(String),
    #[error("no streams found")]
    NoStreams,
    #[error("unknown container format")]
    UnknownFormat,
    #[error("ffprobe: {0}")]
    Ffmpeg(String),
}

#[derive(Debug, thiserror::Error)]
pub enum TranscodeError {
    #[error("input error: {0}")]
    Input(String),
    #[error("output error: {0}")]
    Output(String),
    #[error("no suitable stream found for {0}")]
    NoStream(String),
    #[error("cancelled")]
    Cancelled,
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("unsupported audio codec: {0:?}")]
    UnsupportedAudio(AudioCodec),
    #[error("unsupported video codec: {0:?}")]
    UnsupportedVideo(VideoCodec),
    #[error("codec {0} not available in this ffmpeg build")]
    NotAvailable(String),
    #[error("container {container:?} does not support codec {codec}")]
    IncompatibleContainer { container: Container, codec: String },
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("unknown preset: {0}")]
    Unknown(String),
    #[error("preset incompatible with input: {0}")]
    Incompatible(String),
}

pub type Result<T> = std::result::Result<T, MediaError>;
