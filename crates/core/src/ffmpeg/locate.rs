//! Locate the `ffmpeg`/`ffprobe` binaries and detect their version.

use crate::error::MediaError;
use crate::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolved FFmpeg tooling.
#[derive(Debug, Clone)]
pub struct Tools {
    /// Path to (or bare name of) the `ffmpeg` binary.
    pub ffmpeg: PathBuf,
    /// Path to (or bare name of) the `ffprobe` binary.
    pub ffprobe: PathBuf,
    /// Version string parsed from `ffmpeg -version` (e.g. `"8.1.2"`).
    pub version: String,
}

/// Locate `ffmpeg` and `ffprobe`, honoring explicit overrides, otherwise resolving on
/// `PATH`. Verifies `ffmpeg` is runnable and records its version.
pub fn locate(ffmpeg_override: Option<&Path>, ffprobe_override: Option<&Path>) -> Result<Tools> {
    let ffmpeg = ffmpeg_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"));
    let ffprobe = ffprobe_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("ffprobe"));

    let output = Command::new(&ffmpeg).arg("-version").output().map_err(|e| {
        MediaError::ToolNotFound(format!(
            "could not execute '{}': {e}. Is FFmpeg installed and on PATH?",
            ffmpeg.display()
        ))
    })?;

    if !output.status.success() {
        return Err(MediaError::ToolNotFound(format!(
            "'{}' -version exited with {}",
            ffmpeg.display(),
            output.status
        )));
    }

    let version = parse_version(&String::from_utf8_lossy(&output.stdout))
        .unwrap_or_else(|| "unknown".to_string());

    // Verify ffprobe is runnable too (don't fail hard if only conversion is needed,
    // but probing is core, so surface a clear error early).
    if Command::new(&ffprobe).arg("-version").output().is_err() {
        return Err(MediaError::ToolNotFound(format!(
            "could not execute '{}'. Is ffprobe installed and on PATH?",
            ffprobe.display()
        )));
    }

    Ok(Tools {
        ffmpeg,
        ffprobe,
        version,
    })
}

/// Extract a semantic-ish version from the first line of `ffmpeg -version`.
fn parse_version(text: &str) -> Option<String> {
    // First line looks like: "ffmpeg version 8.1.2 Copyright (c) ...".
    let first = text.lines().next()?;
    let token = first.split_whitespace().nth(2)?;
    Some(token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_from_banner() {
        let text = "ffmpeg version 8.1.2 Copyright (c) 2000-2026 the FFmpeg developers\nbuilt with ...";
        assert_eq!(parse_version(text).as_deref(), Some("8.1.2"));
    }

    #[test]
    fn parse_version_handles_distro_format() {
        let text = "ffmpeg version n6.1.1 Copyright (c)";
        assert_eq!(parse_version(text).as_deref(), Some("n6.1.1"));
    }

    #[test]
    fn parse_version_empty() {
        assert!(parse_version("").is_none());
    }
}
