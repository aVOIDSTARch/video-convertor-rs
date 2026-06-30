//! Structured FFmpeg operations and the request → operation router.
//!
//! Every front-end (CLI and HTTP API) expresses work as a [`crate::UniversalRequest`].
//! [`Operation::from_request`] maps `(method, path, body)` onto a validated operation.
//! The actual execution lives in [`crate::dispatch`].

use crate::codec::{AudioCodec, VideoCodec};
use crate::container::Container;
use crate::error::{MediaError, PresetError};
use crate::ffmpeg::command::Trim;
use crate::format::{AudioSettings, MediaFormat, VideoSettings};
use crate::preset::Preset;
use crate::Result;
use serde::{Deserialize, Serialize};

/// The full, serializable set of conversion options — the single source of truth shared
/// by the CLI flags and the HTTP API body. (Previously this logic lived only in the CLI.)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConvertRequest {
    pub preset: Option<String>,
    /// Target container name (e.g. `"mp4"`). Required unless `preset` is given.
    pub format: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub crf: Option<u8>,
    /// Audio bitrate, accepting `"128k"`, `"192000"`, `"1m"`, etc.
    pub audio_bitrate: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    #[serde(default)]
    pub no_video: bool,
    #[serde(default)]
    pub no_audio: bool,
    #[serde(default)]
    pub copy_video: bool,
    #[serde(default)]
    pub copy_audio: bool,
    pub encoder_preset: Option<String>,
    pub pixel_format: Option<String>,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub duration: Option<f64>,
}

impl ConvertRequest {
    /// The trim window described by this request.
    pub fn trim(&self) -> Trim {
        Trim {
            start: self.start,
            end: self.end,
            duration: self.duration,
        }
    }

    /// Resolve the output container, preferring `format`, then the preset's container,
    /// then `hint` (e.g. derived from an output file extension).
    pub fn resolve_container(&self, hint: Option<Container>) -> Result<Container> {
        if let Some(name) = &self.format {
            return Container::from_name(name)
                .ok_or_else(|| MediaError::UnsupportedFormat(format!("unknown container: {name}")));
        }
        if let Some(preset_name) = &self.preset {
            let p = Preset::by_name(preset_name)
                .ok_or_else(|| PresetError::Unknown(preset_name.clone()))?;
            return Ok(p.format.container);
        }
        hint.ok_or_else(|| {
            MediaError::UnsupportedFormat(
                "no target container: specify `format` or a `preset`".to_string(),
            )
        })
    }

    /// Build the [`MediaFormat`] this request describes.
    pub fn build_format(&self, hint: Option<Container>) -> Result<MediaFormat> {
        if let Some(preset_name) = &self.preset {
            let p = Preset::by_name(preset_name)
                .ok_or_else(|| PresetError::Unknown(preset_name.clone()))?;
            return Ok(p.format.clone());
        }

        let container = self.resolve_container(hint)?;

        let vc = self
            .video_codec
            .as_deref()
            .map(|s| {
                VideoCodec::from_name(s)
                    .ok_or_else(|| MediaError::UnsupportedFormat(format!("unknown video codec: {s}")))
            })
            .transpose()?;
        let ac = self
            .audio_codec
            .as_deref()
            .map(|s| {
                AudioCodec::from_name(s)
                    .ok_or_else(|| MediaError::UnsupportedFormat(format!("unknown audio codec: {s}")))
            })
            .transpose()?;

        let bitrate = self
            .audio_bitrate
            .as_deref()
            .map(parse_bitrate)
            .transpose()?;

        let video = if !self.no_video && !self.copy_video {
            vc.map(|codec| {
                let mut vs = VideoSettings::new(codec);
                vs.crf = self.crf;
                vs.width = self.width;
                vs.height = self.height;
                vs.fps = self.fps;
                vs.encoder_preset = self.encoder_preset.clone();
                vs.pixel_format = self.pixel_format.clone();
                vs
            })
        } else {
            None
        };

        let audio = if !self.no_audio && !self.copy_audio {
            ac.map(|codec| {
                let mut aus = AudioSettings::new(codec);
                aus.bitrate = bitrate;
                aus.sample_rate = self.sample_rate;
                aus.channels = self.channels;
                aus
            })
        } else {
            None
        };

        Ok(MediaFormat {
            container,
            audio,
            video,
            copy_audio: self.copy_audio,
            copy_video: self.copy_video,
            no_audio: self.no_audio,
            no_video: self.no_video,
        })
    }
}

/// Which slice of the capability set to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapKind {
    All,
    Encoders,
    Decoders,
    Filters,
    Muxers,
    Demuxers,
    PixFmts,
    Protocols,
}

impl CapKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "all" | "" => Some(Self::All),
            "encoders" => Some(Self::Encoders),
            "decoders" => Some(Self::Decoders),
            "filters" => Some(Self::Filters),
            "muxers" => Some(Self::Muxers),
            "demuxers" => Some(Self::Demuxers),
            "pix_fmts" | "pixfmts" | "pixel_formats" => Some(Self::PixFmts),
            "protocols" => Some(Self::Protocols),
            _ => None,
        }
    }
}

/// A validated, executable operation.
#[derive(Debug, Clone)]
pub enum Operation {
    /// Full transcode to a resolved [`MediaFormat`].
    Convert { format: MediaFormat, trim: Trim },
    /// Extract the audio track (copy unless `codec` is set).
    ExtractAudio { codec: Option<String>, container: Container },
    /// Single-frame thumbnail at `time` seconds.
    Thumbnail { time: f64, width: Option<u32> },
    /// Apply a single video filtergraph.
    Filter { graph: String, container: Option<Container> },
    /// Concatenate multiple inputs into one output.
    Concat { container: Container },
    /// Probe a single input → JSON (inline, no ffmpeg run).
    Probe,
    /// Report discovered capabilities → JSON (inline).
    Capabilities { kind: CapKind },
    /// List presets → JSON (inline).
    Presets,
    /// Gated raw passthrough: `args` with `INPUT`/`OUTPUT` placeholders.
    Raw { args: Vec<String>, output_ext: String },
}

impl Operation {
    /// Whether this operation produces an inline JSON result rather than an output file.
    pub fn is_inline(&self) -> bool {
        matches!(self, Operation::Probe | Operation::Capabilities { .. } | Operation::Presets)
    }

    /// How many input files this operation consumes (1 unless concat).
    pub fn input_count(&self) -> usize {
        match self {
            Operation::Concat { .. } => 0, // variable; validated by dispatch (>= 2)
            Operation::Presets | Operation::Capabilities { .. } => 0,
            _ => 1,
        }
    }

    /// The output file extension for file-producing operations.
    pub fn output_extension(&self, primary_input_ext: Option<&str>) -> String {
        match self {
            Operation::Convert { format, .. } => format.container.extension().to_string(),
            Operation::ExtractAudio { container, .. } => container.extension().to_string(),
            Operation::Thumbnail { .. } => "jpg".to_string(),
            Operation::Filter { container, .. } => container
                .map(|c| c.extension().to_string())
                .or_else(|| primary_input_ext.map(str::to_string))
                .unwrap_or_else(|| "mkv".to_string()),
            Operation::Concat { container } => container.extension().to_string(),
            Operation::Raw { output_ext, .. } => output_ext.clone(),
            _ => "json".to_string(),
        }
    }

    /// Route a universal request to an operation. Pure parsing/validation; no I/O.
    pub fn from_request(req: &crate::UniversalRequest) -> Result<Operation> {
        let path = normalize_path(&req.path);
        match (req.method, path.as_str()) {
            (crate::Method::Post, "convert") => {
                let cr: ConvertRequest = parse_body(req)?;
                let format = cr.build_format(None)?;
                Ok(Operation::Convert {
                    format,
                    trim: cr.trim(),
                })
            }
            (crate::Method::Post, "extract-audio") => {
                let codec = req.body_str("codec").map(str::to_string);
                let container = req
                    .body_str("format")
                    .and_then(Container::from_name)
                    .unwrap_or(Container::M4a);
                Ok(Operation::ExtractAudio { codec, container })
            }
            (crate::Method::Post, "thumbnail") => {
                let time = req.body_f64("time").unwrap_or(0.0);
                let width = req.body_u64("width").map(|v| v as u32);
                Ok(Operation::Thumbnail { time, width })
            }
            (crate::Method::Post, "filter") => {
                let graph = req
                    .body_str("graph")
                    .ok_or_else(|| MediaError::other("filter requires a `graph`"))?
                    .to_string();
                let container = req.body_str("format").and_then(Container::from_name);
                Ok(Operation::Filter { graph, container })
            }
            (crate::Method::Post, "concat") => {
                let container = req
                    .body_str("format")
                    .and_then(Container::from_name)
                    .unwrap_or(Container::Mkv);
                Ok(Operation::Concat { container })
            }
            (crate::Method::Post, "probe") => Ok(Operation::Probe),
            (crate::Method::Get, "capabilities") => {
                let kind = req
                    .query
                    .get("kind")
                    .map(String::as_str)
                    .and_then(CapKind::parse)
                    .unwrap_or(CapKind::All);
                Ok(Operation::Capabilities { kind })
            }
            (crate::Method::Get, "presets") => Ok(Operation::Presets),
            (crate::Method::Post, "raw") => {
                let args: Vec<String> = req
                    .body
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .ok_or_else(|| MediaError::other("raw requires an `args` array"))?;
                let output_ext = req
                    .body_str("output_ext")
                    .unwrap_or("mkv")
                    .trim_start_matches('.')
                    .to_string();
                Ok(Operation::Raw { args, output_ext })
            }
            (m, p) => Err(MediaError::other(format!("no operation for {m:?} /{p}"))),
        }
    }
}

fn normalize_path(path: &str) -> String {
    path.trim_matches('/')
        .strip_prefix("api/v1/")
        .unwrap_or_else(|| path.trim_matches('/'))
        .trim_matches('/')
        .to_string()
}

fn parse_body<T: for<'de> Deserialize<'de>>(req: &crate::UniversalRequest) -> Result<T> {
    serde_json::from_value(req.body.clone())
        .map_err(|e| MediaError::other(format!("invalid request body: {e}")))
}

/// Parse an audio bitrate string: `"128k"`, `"1m"`, or a raw number.
pub fn parse_bitrate(s: &str) -> Result<u32> {
    let s = s.trim();
    let parse = |num: &str| num.parse::<u32>().map_err(|_| MediaError::other(format!("invalid bitrate: {s}")));
    if let Some(num) = s.strip_suffix(['k', 'K']) {
        Ok(parse(num)? * 1_000)
    } else if let Some(num) = s.strip_suffix(['m', 'M']) {
        Ok(parse(num)? * 1_000_000)
    } else {
        parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_parsing() {
        assert_eq!(parse_bitrate("128k").unwrap(), 128_000);
        assert_eq!(parse_bitrate("192000").unwrap(), 192_000);
        assert_eq!(parse_bitrate("1M").unwrap(), 1_000_000);
        assert!(parse_bitrate("abc").is_err());
    }

    #[test]
    fn build_format_from_preset() {
        let cr = ConvertRequest {
            preset: Some("podcast-mp3".to_string()),
            ..Default::default()
        };
        let fmt = cr.build_format(None).unwrap();
        assert_eq!(fmt.container, Container::Mp3);
        assert!(fmt.audio.is_some());
    }

    #[test]
    fn build_format_from_flags() {
        let cr = ConvertRequest {
            format: Some("mp4".to_string()),
            video_codec: Some("h264".to_string()),
            crf: Some(20),
            audio_codec: Some("aac".to_string()),
            audio_bitrate: Some("128k".to_string()),
            ..Default::default()
        };
        let fmt = cr.build_format(None).unwrap();
        assert_eq!(fmt.container, Container::Mp4);
        assert_eq!(fmt.video.as_ref().unwrap().crf, Some(20));
        assert_eq!(fmt.audio.as_ref().unwrap().bitrate, Some(128_000));
    }

    #[test]
    fn convert_requires_container() {
        let cr = ConvertRequest::default();
        assert!(cr.build_format(None).is_err());
        assert!(cr.build_format(Some(Container::Mkv)).is_ok());
    }

    #[test]
    fn cap_kind_parse() {
        assert_eq!(CapKind::parse("encoders"), Some(CapKind::Encoders));
        assert_eq!(CapKind::parse(""), Some(CapKind::All));
        assert_eq!(CapKind::parse("nope"), None);
    }
}
