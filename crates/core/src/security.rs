//! Security primitives: filename sanitization, path confinement, protocol
//! denylisting, and raw-argument allowlisting.
//!
//! The engine never invokes a shell — arguments are always passed as a vector — so the
//! main attack surface is (a) user-controlled *paths* escaping the managed work dirs,
//! (b) user-controlled *inputs* smuggling an ffmpeg protocol (`http:`, `concat:`,
//! `pipe:`…), and (c) the gated raw passthrough. This module centralizes the checks.

use crate::error::MediaError;
use crate::Result;
use std::path::{Component, Path, PathBuf};

/// Sanitize an uploaded filename: strip any directory components and reject control
/// characters, returning a safe basename. Empty/odd names fall back to `"upload"`.
pub fn sanitize_filename(name: &str) -> String {
    // Take only the final path component, then keep a conservative character set.
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("upload")
        .trim()
        .trim_start_matches('.');

    let cleaned: String = base
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '_',
        })
        .collect();

    let cleaned = cleaned.trim_matches('_').to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "upload".to_string()
    } else {
        cleaned
    }
}

/// Resolve `candidate` and require that it stays within `base` (no traversal,
/// no absolute escape, no symlink escape for existing paths).
///
/// `candidate` may be relative (resolved against `base`) or absolute (must already be
/// under `base`). Returns the confined, lexically-normalized absolute path.
pub fn confine_path(base: &Path, candidate: &Path) -> Result<PathBuf> {
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    };

    let normalized = lexical_normalize(&joined);
    let base_norm = lexical_normalize(base);

    if !normalized.starts_with(&base_norm) {
        return Err(MediaError::security(format!(
            "path '{}' escapes the managed directory '{}'",
            candidate.display(),
            base.display()
        )));
    }

    // If the path (or an ancestor) already exists, defend against symlink escapes by
    // canonicalizing the deepest existing ancestor and re-checking containment.
    if let Some(real_base) = canonical_existing_ancestor(&normalized) {
        let real_base_root =
            canonical_existing_ancestor(&base_norm).unwrap_or_else(|| base_norm.clone());
        if !real_base.starts_with(&real_base_root) {
            return Err(MediaError::security(format!(
                "path '{}' resolves (via symlink) outside '{}'",
                candidate.display(),
                base.display()
            )));
        }
    }

    Ok(normalized)
}

/// Lexically normalize a path (resolve `.` and `..` without touching the filesystem).
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonicalize the deepest existing ancestor of `path` (for symlink-escape checks).
fn canonical_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(p) = current {
        if let Ok(real) = p.canonicalize() {
            return Some(real);
        }
        current = p.parent();
    }
    None
}

/// FFmpeg protocol prefixes that must never appear in a user-supplied *input* string.
/// Local file paths have no scheme, so any `scheme:` token here is rejected.
const DENIED_PROTOCOL_PREFIXES: &[&str] = &[
    "http:", "https:", "tcp:", "udp:", "rtp:", "rtmp:", "rtmps:", "rtsp:", "srt:", "ftp:",
    "ftps:", "sftp:", "gopher:", "tls:", "unix:", "pipe:", "fd:", "concat:", "subfile:",
    "data:", "crypto:", "hls:", "async:", "cache:", "file:", "ffrtmphttp:", "icecast:",
];

/// Reject any user-supplied input/URL that names a non-local ffmpeg protocol.
pub fn ensure_safe_input_token(token: &str) -> Result<()> {
    let lower = token.trim().to_ascii_lowercase();
    for proto in DENIED_PROTOCOL_PREFIXES {
        if lower.starts_with(proto) {
            return Err(MediaError::security(format!(
                "input uses a disallowed protocol: '{token}'"
            )));
        }
    }
    Ok(())
}

/// The protocol whitelist passed to ffmpeg so it refuses anything but local files.
pub const PROTOCOL_WHITELIST: &str = "file,crypto";

/// ffmpeg flags that are *never* allowed in raw passthrough because they enable
/// arbitrary code/config loading, network protocols, or escape the sandbox.
const RAW_DENIED_FLAGS: &[&str] = &[
    // Arbitrary input demuxers / lavfi virtual inputs / device capture.
    "-f", "-fflags", "-protocol_whitelist", "-protocol_blacklist", "-i_qfactor",
    // Loading external configs/scripts.
    "-init_hw_device", "-filter_script", "-filter_complex_script", "-/filter_complex",
    "-vf_script", "-af_script",
];

/// Tokens that signal a network/virtual protocol anywhere in a raw arg list.
fn token_has_denied_protocol(token: &str) -> bool {
    ensure_safe_input_token(token).is_err()
}

/// Validate a raw ffmpeg argument vector against the sandbox policy.
///
/// This is intentionally conservative: raw mode is for power users who have explicitly
/// enabled it, but even then we block network/virtual protocols, config/script loading,
/// and `lavfi`/device formats. I/O paths are confined separately by the caller.
pub fn validate_raw_args(args: &[String]) -> Result<()> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let lower = arg.to_ascii_lowercase();

        if RAW_DENIED_FLAGS.contains(&lower.as_str()) {
            return Err(MediaError::security(format!(
                "raw argument '{arg}' is not permitted"
            )));
        }

        if token_has_denied_protocol(arg) {
            return Err(MediaError::security(format!(
                "raw argument '{arg}' references a disallowed protocol"
            )));
        }

        // Block obvious shell/expansion metacharacters even though we never use a shell.
        if arg.contains('\0') {
            return Err(MediaError::security("raw argument contains a NUL byte".to_string()));
        }

        i += 1;
    }
    Ok(())
}

/// Constant-time-ish bearer token comparison.
pub fn token_matches(expected: &str, provided: &str) -> bool {
    if expected.len() != provided.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in expected.bytes().zip(provided.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn sanitize_strips_directories() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("/abs/path/movie.mp4"), "movie.mp4");
        assert_eq!(sanitize_filename("weird name!.mkv"), "weird_name_.mkv");
        assert_eq!(sanitize_filename(""), "upload");
        assert_eq!(sanitize_filename("..."), "upload");
    }

    #[test]
    fn confine_rejects_traversal() {
        let base = Path::new("/work/output");
        assert!(confine_path(base, Path::new("a/b.mp4")).is_ok());
        assert!(confine_path(base, Path::new("../secrets.txt")).is_err());
        assert!(confine_path(base, Path::new("/etc/passwd")).is_err());
        assert!(confine_path(base, Path::new("a/../../escape")).is_err());
    }

    #[test]
    fn confine_allows_inside() {
        let base = Path::new("/work/output");
        let p = confine_path(base, Path::new("sub/dir/out.mkv")).unwrap();
        assert_eq!(p, Path::new("/work/output/sub/dir/out.mkv"));
    }

    #[test]
    fn denied_protocols_rejected() {
        assert!(ensure_safe_input_token("http://evil/x").is_err());
        assert!(ensure_safe_input_token("concat:a|b").is_err());
        assert!(ensure_safe_input_token("file:/etc/passwd").is_err());
        assert!(ensure_safe_input_token("/home/user/movie.mp4").is_ok());
        assert!(ensure_safe_input_token("movie.mp4").is_ok());
    }

    #[test]
    fn raw_args_block_dangerous() {
        assert!(validate_raw_args(&["-crf".into(), "23".into()]).is_ok());
        assert!(validate_raw_args(&["-f".into(), "lavfi".into()]).is_err());
        assert!(validate_raw_args(&["-i".into(), "http://x".into()]).is_err());
        assert!(validate_raw_args(&["-filter_complex_script".into(), "x".into()]).is_err());
    }

    #[test]
    fn token_compare() {
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc123", "abc124"));
        assert!(!token_matches("abc", "abcd"));
    }
}
