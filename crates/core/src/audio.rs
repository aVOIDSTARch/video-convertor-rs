//! Audio convenience functions: extract, convert, normalize.
//!
//! These are high-level wrappers around the transcode engine for common
//! audio-specific operations.

use crate::codec::AudioCodec;
use crate::container::Container;
use crate::format::{AudioSettings, MediaFormat};
use crate::stream::{InputSource, OutputTarget};
use crate::transcode::TranscodeJob;
use std::path::Path;

/// Build a job to extract audio from a video file (codec copy).
pub fn extract_audio_job(input: impl Into<InputSource>, output: impl Into<OutputTarget>) -> TranscodeJob {
    TranscodeJob::new(input, output, MediaFormat::extract_audio(Container::M4a))
}

/// Build a job to convert audio to a specific codec/container.
pub fn convert_audio_job(
    input: impl Into<InputSource>,
    output: impl Into<OutputTarget>,
    codec: AudioCodec,
    container: Container,
) -> TranscodeJob {
    let settings = AudioSettings::new(codec);
    let format = MediaFormat::audio_only(container, settings);
    TranscodeJob::new(input, output, format)
}

/// Build a job to convert audio with detailed settings.
pub fn convert_audio_job_with_settings(
    input: impl Into<InputSource>,
    output: impl Into<OutputTarget>,
    container: Container,
    settings: AudioSettings,
) -> TranscodeJob {
    let format = MediaFormat::audio_only(container, settings);
    TranscodeJob::new(input, output, format)
}

/// Infer output container from file extension, falling back to the given default.
pub fn infer_audio_container(output_path: &Path, default: Container) -> Container {
    Container::from_path(output_path).unwrap_or(default)
}

// ── FFmpeg-dependent convenience functions ─────────────────────

#[cfg(feature = "ffmpeg")]
mod ffmpeg_audio {
    use super::*;
    use crate::progress::NoProgress;
    use crate::transcode;

    /// Extract audio from a video file to an output path (codec copy).
    pub fn extract_audio(input: &Path, output: &Path) -> crate::Result<TranscodeResult> {
        let container = infer_audio_container(output, Container::M4a);
        let format = MediaFormat::extract_audio(container);
        let job = TranscodeJob::new(
            PathBuf::from(input),
            PathBuf::from(output),
            format,
        );
        transcode::transcode_simple(job)
    }

    /// Convert an audio file to a different format.
    pub fn convert_audio(
        input: &Path,
        output: &Path,
        codec: AudioCodec,
    ) -> crate::Result<TranscodeResult> {
        let container = infer_audio_container(output, Container::Mp3);
        let settings = AudioSettings::new(codec);
        let format = MediaFormat::audio_only(container, settings);
        let job = TranscodeJob::new(
            PathBuf::from(input),
            PathBuf::from(output),
            format,
        );
        transcode::transcode_simple(job)
    }
}

#[cfg(feature = "ffmpeg")]
pub use ffmpeg_audio::{convert_audio, extract_audio};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extract_audio_job_strips_video() {
        let job = extract_audio_job(
            PathBuf::from("/input.mp4"),
            PathBuf::from("/output.m4a"),
        );
        assert!(job.format.no_video);
        assert!(job.format.copy_audio);
    }

    #[test]
    fn convert_audio_job_sets_codec() {
        let job = convert_audio_job(
            PathBuf::from("/input.wav"),
            PathBuf::from("/output.mp3"),
            AudioCodec::Mp3,
            Container::Mp3,
        );
        let audio = job.format.audio.as_ref().unwrap();
        assert_eq!(audio.codec, AudioCodec::Mp3);
        assert!(job.format.no_video);
    }

    #[test]
    fn infer_container_from_extension() {
        assert_eq!(
            infer_audio_container(Path::new("/out.flac"), Container::Mp3),
            Container::Flac
        );
        assert_eq!(
            infer_audio_container(Path::new("/no_ext"), Container::Mp3),
            Container::Mp3
        );
    }
}
