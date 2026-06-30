//! Translate structured requests into safe ffmpeg argument vectors.
//!
//! These functions produce only the *operation-specific* arguments (`-i in … out`); the
//! [`super::Engine`] prepends the global safety flags. Inputs/outputs are passed as
//! `OsString` path values — never interpolated into a shell string.

use crate::container::Container;
use crate::format::MediaFormat;
use std::ffi::OsString;
use std::path::Path;

/// Optional trim window applied to a transcode.
#[derive(Debug, Default, Clone, Copy)]
pub struct Trim {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub duration: Option<f64>,
}

/// Build the ffmpeg args for a full transcode described by a [`MediaFormat`].
pub fn transcode_args(input: &Path, output: &Path, fmt: &MediaFormat, trim: &Trim) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();

    a.push("-i".into());
    a.push(input.into());

    // Trim (output-side seeking for accuracy).
    if let Some(s) = trim.start {
        a.push("-ss".into());
        a.push(fmt_secs(s).into());
    }
    if let Some(e) = trim.end {
        a.push("-to".into());
        a.push(fmt_secs(e).into());
    }
    if let Some(d) = trim.duration {
        a.push("-t".into());
        a.push(fmt_secs(d).into());
    }

    // GIF is special: it uses its own internal encoder; just scale/fps and drop audio.
    if fmt.container == Container::Gif {
        if let Some(vf) = video_filter_chain(fmt) {
            a.push("-vf".into());
            a.push(vf.into());
        }
        a.push("-an".into());
        a.push(output.into());
        return a;
    }

    // ── Video ──────────────────────────────────────────────────
    if fmt.no_video {
        a.push("-vn".into());
    } else if fmt.copy_video {
        a.push("-c:v".into());
        a.push("copy".into());
    } else if let Some(vs) = &fmt.video {
        a.push("-c:v".into());
        a.push(vs.codec.ffmpeg_encoder().into());
        if let Some(crf) = vs.crf {
            a.push("-crf".into());
            a.push(crf.to_string().into());
        }
        if let Some(preset) = &vs.encoder_preset {
            a.push("-preset".into());
            a.push(preset.clone().into());
        }
        if let Some(pf) = &vs.pixel_format {
            a.push("-pix_fmt".into());
            a.push(pf.clone().into());
        }
        if let Some(vf) = video_filter_chain(fmt) {
            a.push("-vf".into());
            a.push(vf.into());
        }
        if let Some(fps) = vs.fps {
            a.push("-r".into());
            a.push(fmt_secs(fps).into());
        }
    }

    // ── Audio ──────────────────────────────────────────────────
    if fmt.no_audio {
        a.push("-an".into());
    } else if fmt.copy_audio {
        a.push("-c:a".into());
        a.push("copy".into());
    } else if let Some(aus) = &fmt.audio {
        a.push("-c:a".into());
        a.push(aus.codec.ffmpeg_encoder().into());
        if let Some(br) = aus.bitrate {
            a.push("-b:a".into());
            a.push(br.to_string().into());
        }
        if let Some(sr) = aus.sample_rate {
            a.push("-ar".into());
            a.push(sr.to_string().into());
        }
        if let Some(ch) = aus.channels {
            a.push("-ac".into());
            a.push(ch.to_string().into());
        }
    }

    a.push(output.into());
    a
}

/// Build a `scale`/`fps` video filter chain string from a format's video settings.
fn video_filter_chain(fmt: &MediaFormat) -> Option<String> {
    let vs = fmt.video.as_ref();
    let (w, h, fps) = match vs {
        Some(v) => (v.width, v.height, v.fps),
        None => (None, None, None),
    };

    let mut parts: Vec<String> = Vec::new();
    match (w, h) {
        (Some(w), Some(h)) => parts.push(format!("scale={w}:{h}")),
        (Some(w), None) => parts.push(format!("scale={w}:-2")),
        (None, Some(h)) => parts.push(format!("scale=-2:{h}")),
        (None, None) => {}
    }
    // For GIF, fps belongs in the filter chain (no -r).
    if fmt.container == Container::Gif {
        if let Some(fps) = fps {
            parts.push(format!("fps={}", fmt_secs(fps)));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Build args for a single-frame thumbnail extraction.
pub fn thumbnail_args(input: &Path, output: &Path, time: f64, width: Option<u32>) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    // Fast input seek for the thumbnail timestamp.
    a.push("-ss".into());
    a.push(fmt_secs(time).into());
    a.push("-i".into());
    a.push(input.into());
    a.push("-frames:v".into());
    a.push("1".into());
    if let Some(w) = width {
        a.push("-vf".into());
        a.push(format!("scale={w}:-2").into());
    }
    a.push(output.into());
    a
}

/// Build args for extracting the audio track. When `codec` is `None`, the stream is
/// copied; otherwise it is re-encoded with the named encoder.
pub fn extract_audio_args(input: &Path, output: &Path, codec: Option<&str>) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    a.push("-i".into());
    a.push(input.into());
    a.push("-vn".into());
    a.push("-c:a".into());
    a.push(codec.unwrap_or("copy").into());
    a.push(output.into());
    a
}

/// Build args for applying a single video filtergraph.
pub fn filter_args(input: &Path, output: &Path, graph: &str) -> Vec<OsString> {
    vec![
        "-i".into(),
        input.into(),
        "-vf".into(),
        graph.into(),
        output.into(),
    ]
}

/// Build args for concatenating inputs listed in a concat-demuxer list file (re-encoding
/// to the output container for safety).
pub fn concat_args(list_file: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        list_file.into(),
        output.into(),
    ]
}

/// Format a float seconds value compactly (no trailing zeros noise).
fn fmt_secs(v: f64) -> String {
    // Use up to 6 decimals, trimming trailing zeros.
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{AudioCodec, VideoCodec};
    use crate::format::{AudioSettings, MediaFormat, VideoSettings};
    use std::path::Path;

    fn strs(args: &[OsString]) -> Vec<String> {
        args.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn audio_transcode_args() {
        let fmt = MediaFormat::audio_only(
            Container::Mp3,
            AudioSettings::new(AudioCodec::Mp3)
                .with_bitrate(128_000)
                .with_sample_rate(44100)
                .with_channels(1),
        );
        let args = strs(&transcode_args(
            Path::new("in.wav"),
            Path::new("out.mp3"),
            &fmt,
            &Trim::default(),
        ));
        assert!(args.contains(&"-c:a".to_string()));
        assert!(args.contains(&"libmp3lame".to_string()));
        assert!(args.contains(&"-b:a".to_string()));
        assert!(args.contains(&"128000".to_string()));
        assert!(args.contains(&"-vn".to_string()));
        assert_eq!(args.last().unwrap(), "out.mp3");
    }

    #[test]
    fn video_transcode_with_scale_and_crf() {
        let fmt = MediaFormat::video(
            Container::Mp4,
            VideoSettings::new(VideoCodec::H264).with_crf(23).with_resolution(1280, 720),
            AudioSettings::new(AudioCodec::Aac).with_bitrate(128_000),
        );
        let args = strs(&transcode_args(
            Path::new("in.mov"),
            Path::new("out.mp4"),
            &fmt,
            &Trim::default(),
        ));
        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"-crf".to_string()));
        assert!(args.contains(&"23".to_string()));
        assert!(args.iter().any(|a| a == "scale=1280:720"));
    }

    #[test]
    fn copy_modes() {
        let mut fmt = MediaFormat::remux(Container::Mkv);
        fmt.copy_video = true;
        fmt.copy_audio = true;
        let args = strs(&transcode_args(
            Path::new("in.mp4"),
            Path::new("out.mkv"),
            &fmt,
            &Trim::default(),
        ));
        let joined = args.join(" ");
        assert!(joined.contains("-c:v copy"));
        assert!(joined.contains("-c:a copy"));
    }

    #[test]
    fn trim_applied() {
        let fmt = MediaFormat::remux(Container::Mkv);
        let trim = Trim {
            start: Some(1.5),
            end: Some(10.0),
            duration: None,
        };
        let args = strs(&transcode_args(Path::new("i"), Path::new("o.mkv"), &fmt, &trim));
        assert!(args.contains(&"-ss".to_string()));
        assert!(args.contains(&"1.5".to_string()));
        assert!(args.contains(&"-to".to_string()));
    }

    #[test]
    fn gif_has_no_codec_flag() {
        let fmt = MediaFormat {
            container: Container::Gif,
            audio: None,
            video: Some(VideoSettings::new(VideoCodec::H264).with_fps(15.0)),
            copy_audio: false,
            copy_video: false,
            no_audio: true,
            no_video: false,
        };
        let args = strs(&transcode_args(Path::new("i.mp4"), Path::new("o.gif"), &fmt, &Trim::default()));
        assert!(!args.contains(&"-c:v".to_string()));
        assert!(args.contains(&"-an".to_string()));
    }
}
