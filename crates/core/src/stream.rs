//! Custom I/O adapter bridging Rust Read/Write to FFmpeg.
//!
//! Provides `InputSource` and `OutputTarget` enums for flexible I/O:
//! file paths, in-memory buffers, or streaming Read/Write.

use std::io::{Read, Write};
use std::path::PathBuf;

/// Where to read input media from.
pub enum InputSource {
    /// Read from a file path.
    File(PathBuf),
    /// Read from a byte buffer (entire file in memory).
    Bytes(Vec<u8>),
    /// Read from a streaming source.
    Reader(Box<dyn Read + Send>),
}

impl std::fmt::Debug for InputSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(p) => write!(f, "File({})", p.display()),
            Self::Bytes(b) => write!(f, "Bytes({} bytes)", b.len()),
            Self::Reader(_) => write!(f, "Reader(...)"),
        }
    }
}

impl From<PathBuf> for InputSource {
    fn from(path: PathBuf) -> Self {
        Self::File(path)
    }
}

impl From<Vec<u8>> for InputSource {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes(bytes)
    }
}

/// Where to write output media to.
pub enum OutputTarget {
    /// Write to a file path.
    File(PathBuf),
    /// Write to an in-memory buffer (returned in `TranscodeResult`).
    Buffer,
    /// Write to a streaming sink.
    Writer(Box<dyn Write + Send>),
}

impl std::fmt::Debug for OutputTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(p) => write!(f, "File({})", p.display()),
            Self::Buffer => write!(f, "Buffer"),
            Self::Writer(_) => write!(f, "Writer(...)"),
        }
    }
}

impl From<PathBuf> for OutputTarget {
    fn from(path: PathBuf) -> Self {
        Self::File(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_source_debug() {
        let f = InputSource::File(PathBuf::from("/test.mp4"));
        assert!(format!("{:?}", f).contains("test.mp4"));

        let b = InputSource::Bytes(vec![0u8; 100]);
        assert!(format!("{:?}", b).contains("100 bytes"));

        let r = InputSource::Reader(Box::new(std::io::empty()));
        assert!(format!("{:?}", r).contains("Reader"));
    }

    #[test]
    fn output_target_debug() {
        let f = OutputTarget::File(PathBuf::from("/out.mp4"));
        assert!(format!("{:?}", f).contains("out.mp4"));

        assert!(format!("{:?}", OutputTarget::Buffer).contains("Buffer"));
    }

    #[test]
    fn from_conversions() {
        let _: InputSource = PathBuf::from("/test.mp4").into();
        let _: InputSource = vec![0u8; 10].into();
        let _: OutputTarget = PathBuf::from("/out.mp4").into();
    }
}
