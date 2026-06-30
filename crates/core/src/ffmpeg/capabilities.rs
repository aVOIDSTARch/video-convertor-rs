//! Runtime discovery of the installed FFmpeg's capability set.
//!
//! Rather than hand-maintain enums (the installed ffmpeg exposes ~195 encoders and ~488
//! filters), we parse `ffmpeg -encoders/-decoders/-filters/-muxers/-demuxers/-pix_fmts/
//! -protocols` once and expose the result. New ffmpeg builds gain new capabilities for
//! free.

use super::locate::Tools;
use crate::error::MediaError;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Media kind for a codec entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Video,
    Audio,
    Subtitle,
    Other,
}

/// A single encoder or decoder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodecEntry {
    pub name: String,
    pub kind: MediaKind,
    pub description: String,
}

/// A single filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterEntry {
    pub name: String,
    /// e.g. `"V->V"`, `"A->A"`, `"|->V"`.
    pub io: String,
    pub description: String,
}

/// The full discovered capability set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub version: String,
    pub encoders: Vec<CodecEntry>,
    pub decoders: Vec<CodecEntry>,
    pub filters: Vec<FilterEntry>,
    pub muxers: Vec<String>,
    pub demuxers: Vec<String>,
    pub pix_fmts: Vec<String>,
    pub protocols_input: Vec<String>,
    pub protocols_output: Vec<String>,
}

impl Capabilities {
    /// Whether an encoder with the exact `name` is available.
    pub fn has_encoder(&self, name: &str) -> bool {
        self.encoders.iter().any(|e| e.name == name)
    }

    /// Whether a decoder with the exact `name` is available.
    pub fn has_decoder(&self, name: &str) -> bool {
        self.decoders.iter().any(|e| e.name == name)
    }

    /// Whether a filter with the exact `name` is available.
    pub fn has_filter(&self, name: &str) -> bool {
        self.filters.iter().any(|f| f.name == name)
    }

    /// Whether a muxer with the exact `name` is available.
    pub fn has_muxer(&self, name: &str) -> bool {
        self.muxers.iter().any(|m| m == name)
    }

    /// Encoders of a given media kind.
    pub fn encoders_of(&self, kind: MediaKind) -> impl Iterator<Item = &CodecEntry> {
        self.encoders.iter().filter(move |e| e.kind == kind)
    }
}

/// Discover capabilities by invoking the ffmpeg binary several times.
pub fn discover(tools: &Tools) -> Result<Capabilities> {
    let encoders = parse_codecs(&run_list(tools, "-encoders")?);
    let decoders = parse_codecs(&run_list(tools, "-decoders")?);
    let filters = parse_filters(&run_list(tools, "-filters")?);
    let (muxers, _) = parse_format_list(&run_list(tools, "-muxers")?, b'E');
    let (demuxers, _) = parse_format_list(&run_list(tools, "-demuxers")?, b'D');
    let pix_fmts = parse_pix_fmts(&run_list(tools, "-pix_fmts")?);
    let (protocols_input, protocols_output) = parse_protocols(&run_list(tools, "-protocols")?);

    Ok(Capabilities {
        version: tools.version.clone(),
        encoders,
        decoders,
        filters,
        muxers,
        demuxers,
        pix_fmts,
        protocols_input,
        protocols_output,
    })
}

fn run_list(tools: &Tools, flag: &str) -> Result<String> {
    let output = Command::new(&tools.ffmpeg)
        .args(["-hide_banner", flag])
        .output()
        .map_err(|e| MediaError::Ffmpeg(format!("failed to run ffmpeg {flag}: {e}")))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse `-encoders`/`-decoders` output.
///
/// Lines look like: ` V....D libx264   libx264 H.264 / AVC ...`. The first column is a
/// flag block whose first character is the media kind (V/A/S).
fn parse_codecs(text: &str) -> Vec<CodecEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        // Skip header lines until we hit the ` ----` separator block; entries start with
        // a space then the flag column.
        let bytes = line.as_bytes();
        if bytes.len() < 8 || bytes[0] != b' ' {
            continue;
        }
        let flags = &line[1..7];
        let kind = match flags.chars().next() {
            Some('V') => MediaKind::Video,
            Some('A') => MediaKind::Audio,
            Some('S') => MediaKind::Subtitle,
            _ => continue,
        };
        let rest = line[7..].trim_start();
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = match parts.next() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        let description = parts.next().unwrap_or("").trim().to_string();
        out.push(CodecEntry {
            name,
            kind,
            description,
        });
    }
    out
}

/// Parse `-filters` output. Lines: ` T.. name  in->out  description`.
fn parse_filters(text: &str) -> Vec<FilterEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let bytes = line.as_bytes();
        // Layout: one leading space, a 3-char flag column, a space, then name/io/desc.
        if bytes.len() < 6 || bytes[0] != b' ' {
            continue;
        }
        let rest = line[4..].trim_start();
        let mut parts = rest.split_whitespace();
        let name = match parts.next() {
            Some(n) if !n.is_empty() && n != "=" => n.to_string(),
            _ => continue,
        };
        let io = parts.next().unwrap_or("").to_string();
        let description = parts.collect::<Vec<_>>().join(" ");
        out.push(FilterEntry {
            name,
            io,
            description,
        });
    }
    out
}

/// Parse `-muxers`/`-demuxers` output. Lines: ` E name  description` (muxers) or
/// ` D name  description` (demuxers). Returns (names, descriptions-unused).
fn parse_format_list(text: &str, _marker: u8) -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    for line in text.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 4 || bytes[0] != b' ' {
            continue;
        }
        let rest = line[1..].trim_start();
        // First token after the flag column is the name; but the flag column is 1-2 chars.
        // Robust approach: split, the name is the first token that isn't a flag glyph.
        let mut parts = rest.split_whitespace();
        if let Some(first) = parts.next() {
            // `first` may be the flag (e.g. "E" or "DE"); if it's all flag chars, take next.
            let name = if first.chars().all(|c| "DE".contains(c)) {
                parts.next().map(|s| s.to_string())
            } else {
                Some(first.to_string())
            };
            if let Some(n) = name {
                if !n.is_empty() {
                    names.push(n);
                }
            }
        }
    }
    (names, Vec::new())
}

/// Parse `-pix_fmts` output, collecting format names (skip the header block).
fn parse_pix_fmts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_body = false;
    for line in text.lines() {
        if line.starts_with("-----") {
            in_body = true;
            continue;
        }
        if !in_body {
            continue;
        }
        let mut parts = line.split_whitespace();
        // Format: `IO... name  NB_COMPONENTS  BITS_PER_PIXEL`.
        if let (Some(_flags), Some(name)) = (parts.next(), parts.next()) {
            out.push(name.to_string());
        }
    }
    out
}

/// Parse `-protocols` output into (input, output) protocol lists.
fn parse_protocols(text: &str) -> (Vec<String>, Vec<String>) {
    let mut input = Vec::new();
    let mut output = Vec::new();
    let mut section = 0; // 0 = none, 1 = input, 2 = output
    for line in text.lines() {
        let t = line.trim();
        if t.eq_ignore_ascii_case("Input:") {
            section = 1;
            continue;
        }
        if t.eq_ignore_ascii_case("Output:") {
            section = 2;
            continue;
        }
        if t.is_empty() || t.contains(':') {
            continue;
        }
        match section {
            1 => input.push(t.to_string()),
            2 => output.push(t.to_string()),
            _ => {}
        }
    }
    (input, output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_encoders_sample() {
        let sample = "\
Encoders:
 V..... = Video
 ------
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC
 A....D aac                  AAC (Advanced Audio Coding)
 S..... ass                  ASS (Advanced SubStation Alpha) subtitle
";
        let codecs = parse_codecs(sample);
        assert!(codecs.iter().any(|c| c.name == "libx264" && c.kind == MediaKind::Video));
        assert!(codecs.iter().any(|c| c.name == "aac" && c.kind == MediaKind::Audio));
        assert!(codecs.iter().any(|c| c.name == "ass" && c.kind == MediaKind::Subtitle));
    }

    #[test]
    fn parse_filters_sample() {
        let sample = "\
Filters:
 T.. = Timeline support
 ... scale            V->V       Scale the input video size.
 ... aresample        A->A       Resample audio data.
";
        let filters = parse_filters(sample);
        assert!(filters.iter().any(|f| f.name == "scale" && f.io == "V->V"));
        assert!(filters.iter().any(|f| f.name == "aresample"));
    }

    #[test]
    fn parse_protocols_sections() {
        let sample = "Supported file protocols:\nInput:\n  file\n  http\nOutput:\n  file\n  md5\n";
        let (i, o) = parse_protocols(sample);
        assert!(i.contains(&"file".to_string()));
        assert!(i.contains(&"http".to_string()));
        assert!(o.contains(&"md5".to_string()));
    }
}
