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
    #[error("filter error: {0}")]
    Filter(#[from] FilterError),
    #[error("preset error: {0}")]
    Preset(#[from] PresetError),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ffmpeg error: {0}")]
    Ffmpeg(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("cannot open file: {0}")]
    OpenFailed(String),
    #[error("no streams found")]
    NoStreams,
    #[error("unknown container format")]
    UnknownFormat,
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
}

#[derive(Debug, thiserror::Error)]
pub enum TranscodeError {
    #[error("input error: {0}")]
    Input(String),
    #[error("output error: {0}")]
    Output(String),
    #[error("decoder error: {0}")]
    Decoder(String),
    #[error("encoder error: {0}")]
    Encoder(String),
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
    #[error("codec {0} not available in this build")]
    NotAvailable(String),
    #[error("container {container:?} does not support codec {codec}")]
    IncompatibleContainer {
        container: Container,
        codec: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("invalid filter specification: {0}")]
    InvalidSpec(String),
    #[error("filter graph error: {0}")]
    GraphError(String),
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("unknown preset: {0}")]
    Unknown(String),
    #[error("preset incompatible with input: {0}")]
    Incompatible(String),
}

pub type Result<T> = std::result::Result<T, MediaError>;
