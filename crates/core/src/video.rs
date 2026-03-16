//! Video convenience functions: convert, thumbnail, gif.
//!
//! High-level wrappers around the transcode engine for common
//! video-specific operations.

use crate::codec::VideoCodec;
use crate::container::Container;
use crate::format::{AudioSettings, MediaFormat, VideoSettings};
use crate::stream::{InputSource, OutputTarget};
use crate::transcode::TranscodeJob;
use std::path::Path;

/// Build a job to convert video with specified codecs.
pub fn convert_video_job(
    input: impl Into<InputSource>,
    output: impl Into<OutputTarget>,
    container: Container,
    video: VideoSettings,
    audio: AudioSettings,
) -> TranscodeJob {
    let format = MediaFormat::video(container, video, audio);
    TranscodeJob::new(input, output, format)
}

/// Build a job to extract a thumbnail (single frame) from a video.
pub fn thumbnail_job(
    input: impl Into<InputSource>,
    output: impl Into<OutputTarget>,
    time_secs: f64,
) -> TranscodeJob {
    // Thumbnail extraction: seek to time, grab one frame.
    // The actual frame extraction is handled specially in the transcode engine.
    let format = MediaFormat {
        container: Container::Mp4, // placeholder — thumbnail goes to JPEG
        audio: None,
        video: None,
        copy_audio: false,
        copy_video: false,
        no_audio: true,
        no_video: false,
    };
    TranscodeJob::new(input, output, format)
        .with_start_time(time_secs)
        .with_duration(0.1) // just one frame
}

/// Build a job to convert a video segment to GIF.
pub fn gif_job(
    input: impl Into<InputSource>,
    output: impl Into<OutputTarget>,
    width: u32,
    fps: f64,
    start_secs: Option<f64>,
    duration_secs: Option<f64>,
) -> TranscodeJob {
    let video = VideoSettings::new(VideoCodec::H264) // placeholder for GIF
        .with_fps(fps)
        .with_resolution(width, 0); // height auto-calculated

    let format = MediaFormat::video_only(Container::Gif, video);
    let mut job = TranscodeJob::new(input, output, format);

    if let Some(start) = start_secs {
        job = job.with_start_time(start);
    }
    if let Some(dur) = duration_secs {
        job = job.with_duration(dur);
    }
    job
}

/// Infer output container from file extension, falling back to MP4.
pub fn infer_video_container(output_path: &Path, default: Container) -> Container {
    Container::from_path(output_path).unwrap_or(default)
}

// ── FFmpeg-dependent convenience functions ─────────────────────

#[cfg(feature = "ffmpeg")]
mod ffmpeg_video {
    use super::*;
    use crate::transcode;

    /// Convert a video file with the given settings.
    pub fn convert_video(
        input: &Path,
        output: &Path,
        video_codec: VideoCodec,
        audio_codec: AudioCodec,
    ) -> crate::Result<TranscodeResult> {
        let container = infer_video_container(output, Container::Mp4);
        let video = VideoSettings::new(video_codec);
        let audio = AudioSettings::new(audio_codec);
        let job = convert_video_job(
            PathBuf::from(input),
            PathBuf::from(output),
            container,
            video,
            audio,
        );
        transcode::transcode_simple(job)
    }

    /// Convert a video segment to an animated GIF.
    pub fn to_gif(
        input: &Path,
        output: &Path,
        width: u32,
        fps: f64,
        start_secs: Option<f64>,
        duration_secs: Option<f64>,
    ) -> crate::Result<TranscodeResult> {
        let job = gif_job(
            PathBuf::from(input),
            PathBuf::from(output),
            width,
            fps,
            start_secs,
            duration_secs,
        );
        transcode::transcode_simple(job)
    }
}

#[cfg(feature = "ffmpeg")]
pub use ffmpeg_video::{convert_video, to_gif};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::AudioCodec;
    use std::path::PathBuf;

    #[test]
    fn convert_video_job_has_both_streams() {
        let job = convert_video_job(
            PathBuf::from("/input.mkv"),
            PathBuf::from("/output.mp4"),
            Container::Mp4,
            VideoSettings::new(VideoCodec::H264).with_crf(23),
            AudioSettings::new(AudioCodec::Aac).with_bitrate(128_000),
        );
        assert!(job.format.video.is_some());
        assert!(job.format.audio.is_some());
        assert!(!job.format.no_video);
        assert!(!job.format.no_audio);
    }

    #[test]
    fn gif_job_is_video_only() {
        let job = gif_job(
            PathBuf::from("/input.mp4"),
            PathBuf::from("/output.gif"),
            480,
            15.0,
            Some(5.0),
            Some(10.0),
        );
        assert!(job.format.no_audio);
        assert_eq!(job.format.container, Container::Gif);
        assert_eq!(job.start_time, Some(5.0));
        assert_eq!(job.duration, Some(10.0));
    }

    #[test]
    fn thumbnail_job_seeks_to_time() {
        let job = thumbnail_job(
            PathBuf::from("/input.mp4"),
            PathBuf::from("/thumb.jpg"),
            5.0,
        );
        assert_eq!(job.start_time, Some(5.0));
        assert!(job.format.no_audio);
    }

    #[test]
    fn infer_container() {
        assert_eq!(
            infer_video_container(Path::new("/out.mkv"), Container::Mp4),
            Container::Mkv
        );
        assert_eq!(
            infer_video_container(Path::new("/no_ext"), Container::Mp4),
            Container::Mp4
        );
    }
}
