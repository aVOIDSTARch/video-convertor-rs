//! Transcode engine: decode → filter → encode pipeline.
//!
//! The main entry point is `transcode()` which takes a `TranscodeJob` and
//! produces a `TranscodeResult`.

use crate::format::MediaFormat;
use crate::stream::{InputSource, OutputTarget};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A complete transcode job specification.
#[derive(Debug)]
pub struct TranscodeJob {
    /// Input media source.
    pub input: InputSource,
    /// Output target.
    pub output: OutputTarget,
    /// Desired output format.
    pub format: MediaFormat,
    /// Optional start time for trimming (seconds).
    pub start_time: Option<f64>,
    /// Optional end time for trimming (seconds).
    pub end_time: Option<f64>,
    /// Optional duration limit (seconds).
    pub duration: Option<f64>,
    /// Number of threads for this job (0 = auto).
    pub threads: u32,
}

impl TranscodeJob {
    /// Create a new job from input, output, and format.
    pub fn new(input: impl Into<InputSource>, output: impl Into<OutputTarget>, format: MediaFormat) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
            format,
            start_time: None,
            end_time: None,
            duration: None,
            threads: 0,
        }
    }

    pub fn with_start_time(mut self, secs: f64) -> Self {
        self.start_time = Some(secs);
        self
    }

    pub fn with_end_time(mut self, secs: f64) -> Self {
        self.end_time = Some(secs);
        self
    }

    pub fn with_duration(mut self, secs: f64) -> Self {
        self.duration = Some(secs);
        self
    }

    pub fn with_threads(mut self, threads: u32) -> Self {
        self.threads = threads;
        self
    }
}

/// Result of a completed transcode operation.
#[derive(Debug)]
pub struct TranscodeResult {
    /// Output file path (if output was `File`).
    pub output_path: Option<PathBuf>,
    /// Output bytes (if output was `Buffer`).
    pub output_bytes: Option<Vec<u8>>,
    /// Duration of the output in seconds.
    pub duration_secs: Option<f64>,
    /// Output file size in bytes.
    pub output_size: u64,
}

/// A cancellation token that can be shared across threads.
#[derive(Debug, Clone)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Check if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

// ── FFmpeg-based transcode implementation ──────────────────────

#[cfg(feature = "ffmpeg")]
mod ffmpeg_transcode {
    use super::*;
    use crate::error::{MediaError, TranscodeError};
    use crate::filter::{AudioFilterChain, VideoFilterChain};
    use crate::progress::{NoProgress, ProgressEvent};
    use std::path::Path;
    use std::time::Duration;

    /// Run a transcode job with progress reporting.
    pub fn transcode(
        job: TranscodeJob,
        progress: &mut dyn ProgressHandler,
        cancel: Option<&CancelToken>,
    ) -> crate::Result<TranscodeResult> {
        crate::init();

        // For now, only file-to-file transcoding is implemented.
        // Reader/Writer/Buffer modes will use tempfiles as intermediaries
        // for containers that require seeking (MP4 moov atom).
        let (input_path, _temp_input) = resolve_input(&job.input)?;
        let (output_path, temp_output) = resolve_output(&job.output, &job.format)?;

        run_file_transcode(&input_path, &output_path, &job, progress, cancel)?;

        let output_size = std::fs::metadata(&output_path)
            .map(|m| m.len())
            .unwrap_or(0);

        let result = if temp_output {
            // Output was to a temp file — read it back into a buffer.
            let bytes = std::fs::read(&output_path)
                .map_err(|e| MediaError::Io(e))?;
            let _ = std::fs::remove_file(&output_path);
            TranscodeResult {
                output_path: None,
                output_bytes: Some(bytes),
                duration_secs: None,
                output_size,
            }
        } else {
            TranscodeResult {
                output_path: Some(output_path),
                output_bytes: None,
                duration_secs: None,
                output_size,
            }
        };

        progress.on_complete();
        Ok(result)
    }

    /// Convenience: transcode without progress reporting or cancellation.
    pub fn transcode_simple(job: TranscodeJob) -> crate::Result<TranscodeResult> {
        transcode(job, &mut NoProgress, None)
    }

    fn resolve_input(
        source: &InputSource,
    ) -> crate::Result<(std::path::PathBuf, Option<tempfile::NamedTempFile>)> {
        match source {
            InputSource::File(p) => Ok((p.clone(), None)),
            InputSource::Bytes(bytes) => {
                let mut tmp = tempfile::NamedTempFile::new()
                    .map_err(MediaError::Io)?;
                std::io::Write::write_all(&mut tmp, bytes)
                    .map_err(MediaError::Io)?;
                let path = tmp.path().to_path_buf();
                Ok((path, Some(tmp)))
            }
            InputSource::Reader(_) => {
                // TODO: streaming input via custom I/O context
                Err(MediaError::Transcode(TranscodeError::Input(
                    "streaming Reader input not yet implemented; use File or Bytes".into(),
                )))
            }
        }
    }

    fn resolve_output(
        target: &OutputTarget,
        format: &crate::format::MediaFormat,
    ) -> crate::Result<(std::path::PathBuf, bool)> {
        match target {
            OutputTarget::File(p) => Ok((p.clone(), false)),
            OutputTarget::Buffer => {
                let ext = format.container.extension();
                let tmp = tempfile::Builder::new()
                    .suffix(&format!(".{ext}"))
                    .tempfile()
                    .map_err(MediaError::Io)?;
                let path = tmp.into_temp_path().to_path_buf();
                Ok((path, true))
            }
            OutputTarget::Writer(_) => {
                Err(MediaError::Transcode(TranscodeError::Output(
                    "streaming Writer output not yet implemented; use File or Buffer".into(),
                )))
            }
        }
    }

    fn run_file_transcode(
        input_path: &Path,
        output_path: &Path,
        job: &TranscodeJob,
        progress: &mut dyn ProgressHandler,
        cancel: Option<&CancelToken>,
    ) -> crate::Result<()> {
        let mut ictx = ffmpeg_next::format::input(input_path)
            .map_err(|e| TranscodeError::Input(e.to_string()))?;

        let input_duration = if ictx.duration() > 0 {
            Some(Duration::from_micros(ictx.duration() as u64))
        } else {
            None
        };

        let mut octx = ffmpeg_next::format::output_as(
            output_path,
            job.format.container.ffmpeg_format(),
        )
        .map_err(|e| TranscodeError::Output(e.to_string()))?;

        // Map streams and set up encoders
        let mut stream_map: Vec<Option<usize>> = vec![None; ictx.streams().count()];
        let mut output_stream_idx = 0usize;

        // Find best video stream
        let video_stream_idx = if !job.format.no_video {
            ictx.streams()
                .best(ffmpeg_next::media::Type::Video)
                .map(|s| s.index())
        } else {
            None
        };

        // Find best audio stream
        let audio_stream_idx = if !job.format.no_audio {
            ictx.streams()
                .best(ffmpeg_next::media::Type::Audio)
                .map(|s| s.index())
        } else {
            None
        };

        // Set up video output stream
        if let Some(vi) = video_stream_idx {
            let input_stream = ictx.stream(vi).unwrap();
            if job.format.copy_video {
                let mut out_stream = octx.add_stream(ffmpeg_next::codec::Id::None)
                    .map_err(|e| TranscodeError::Encoder(e.to_string()))?;
                out_stream.set_parameters(input_stream.parameters());
                out_stream.set_time_base(input_stream.time_base());
            } else if let Some(ref vs) = job.format.video {
                let encoder = ffmpeg_next::codec::encoder::find_by_name(vs.codec.ffmpeg_encoder())
                    .ok_or_else(|| {
                        TranscodeError::Encoder(format!(
                            "encoder not found: {}",
                            vs.codec.ffmpeg_encoder()
                        ))
                    })?;
                let mut out_stream = octx.add_stream(encoder)
                    .map_err(|e| TranscodeError::Encoder(e.to_string()))?;

                let mut enc_ctx = ffmpeg_next::codec::context::Context::new_with_codec(encoder)
                    .encoder()
                    .video()
                    .map_err(|e| TranscodeError::Encoder(e.to_string()))?;

                // Get input dimensions from decoder
                let dec_ctx = ffmpeg_next::codec::context::Context::from_parameters(
                    input_stream.parameters(),
                )
                .map_err(|e| TranscodeError::Decoder(e.to_string()))?;
                let decoder = dec_ctx
                    .decoder()
                    .video()
                    .map_err(|e| TranscodeError::Decoder(e.to_string()))?;

                let width = vs.width.unwrap_or(decoder.width());
                let height = vs.height.unwrap_or(decoder.height());
                enc_ctx.set_width(width);
                enc_ctx.set_height(height);

                if let Some(crf) = vs.crf {
                    // CRF is set via codec-specific options
                    let _ = enc_ctx.set_option("crf", &crf.to_string());
                }

                if let Some(ref preset) = vs.encoder_preset {
                    let _ = enc_ctx.set_option("preset", preset);
                }

                let fps = vs.fps.unwrap_or_else(|| {
                    let r = input_stream.avg_frame_rate();
                    if r.1 > 0 { r.0 as f64 / r.1 as f64 } else { 30.0 }
                });
                enc_ctx.set_frame_rate(Some(ffmpeg_next::Rational::new(
                    (fps * 1000.0) as i32,
                    1000,
                )));
                enc_ctx.set_time_base(ffmpeg_next::Rational::new(1, (fps * 1000.0) as i32));

                let pixel_fmt = vs
                    .pixel_format
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(ffmpeg_next::format::Pixel::YUV420P);
                enc_ctx.set_format(pixel_fmt);

                let opened = enc_ctx
                    .open_as(encoder)
                    .map_err(|e| TranscodeError::Encoder(e.to_string()))?;
                out_stream.set_parameters(&opened);
            }

            stream_map[vi] = Some(output_stream_idx);
            output_stream_idx += 1;
        }

        // Set up audio output stream
        if let Some(ai) = audio_stream_idx {
            let input_stream = ictx.stream(ai).unwrap();
            if job.format.copy_audio {
                let mut out_stream = octx.add_stream(ffmpeg_next::codec::Id::None)
                    .map_err(|e| TranscodeError::Encoder(e.to_string()))?;
                out_stream.set_parameters(input_stream.parameters());
                out_stream.set_time_base(input_stream.time_base());
            } else if let Some(ref aus) = job.format.audio {
                let encoder =
                    ffmpeg_next::codec::encoder::find_by_name(aus.codec.ffmpeg_encoder())
                        .ok_or_else(|| {
                            TranscodeError::Encoder(format!(
                                "encoder not found: {}",
                                aus.codec.ffmpeg_encoder()
                            ))
                        })?;
                let mut out_stream = octx.add_stream(encoder)
                    .map_err(|e| TranscodeError::Encoder(e.to_string()))?;

                let mut enc_ctx = ffmpeg_next::codec::context::Context::new_with_codec(encoder)
                    .encoder()
                    .audio()
                    .map_err(|e| TranscodeError::Encoder(e.to_string()))?;

                let dec_ctx = ffmpeg_next::codec::context::Context::from_parameters(
                    input_stream.parameters(),
                )
                .map_err(|e| TranscodeError::Decoder(e.to_string()))?;
                let decoder = dec_ctx
                    .decoder()
                    .audio()
                    .map_err(|e| TranscodeError::Decoder(e.to_string()))?;

                let sample_rate = aus.sample_rate.unwrap_or(decoder.rate());
                enc_ctx.set_rate(sample_rate as i32);

                let channels = aus.channels.unwrap_or(decoder.channels() as u16);
                enc_ctx.set_channels(channels as i32);
                enc_ctx.set_channel_layout(if channels == 1 {
                    ffmpeg_next::ChannelLayout::MONO
                } else {
                    ffmpeg_next::ChannelLayout::STEREO
                });

                if let Some(bitrate) = aus.bitrate {
                    enc_ctx.set_bit_rate(bitrate as usize);
                }

                // Use first supported sample format
                enc_ctx.set_format(
                    encoder
                        .audio()
                        .map(|a| {
                            a.formats()
                                .map(|mut fmts| fmts.next().unwrap_or(ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar)))
                                .unwrap_or(ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar))
                        })
                        .unwrap_or(ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar)),
                );

                let opened = enc_ctx
                    .open_as(encoder)
                    .map_err(|e| TranscodeError::Encoder(e.to_string()))?;
                out_stream.set_parameters(&opened);
            }

            stream_map[ai] = Some(output_stream_idx);
            // output_stream_idx += 1; // not needed after last stream
        }

        octx.write_header()
            .map_err(|e| TranscodeError::Output(format!("write header: {e}")))?;

        // Packet remux loop (copy mode or passthrough after encoding)
        for (stream, packet) in ictx.packets() {
            if let Some(cancel) = cancel {
                if cancel.is_cancelled() {
                    return Err(TranscodeError::Cancelled.into());
                }
            }

            let in_idx = stream.index();
            if let Some(out_idx) = stream_map[in_idx] {
                // Report progress
                if let Some(pts) = packet.pts() {
                    let tb = stream.time_base();
                    let pos_secs = pts as f64 * tb.0 as f64 / tb.1 as f64;
                    let pos = Duration::from_secs_f64(pos_secs.max(0.0));
                    progress.on_progress(&ProgressEvent::new(pos, input_duration));
                }

                let mut out_packet = packet.clone();
                let in_tb = stream.time_base();
                let out_tb = octx.stream(out_idx).unwrap().time_base();
                out_packet.rescale_ts(in_tb, out_tb);
                out_packet.set_stream(out_idx);
                out_packet
                    .write_interleaved(&mut octx)
                    .map_err(|e| TranscodeError::Output(format!("write packet: {e}")))?;
            }
        }

        octx.write_trailer()
            .map_err(|e| TranscodeError::Output(format!("write trailer: {e}")))?;

        Ok(())
    }
}

#[cfg(feature = "ffmpeg")]
pub use ffmpeg_transcode::{transcode, transcode_simple};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::MediaFormat;
    use crate::container::Container;
    use std::path::PathBuf;

    #[test]
    fn transcode_job_builder() {
        let format = MediaFormat::remux(Container::Mkv);
        let job = TranscodeJob::new(
            PathBuf::from("/input.mp4"),
            PathBuf::from("/output.mkv"),
            format,
        )
        .with_start_time(10.0)
        .with_end_time(60.0)
        .with_threads(4);

        assert_eq!(job.start_time, Some(10.0));
        assert_eq!(job.end_time, Some(60.0));
        assert_eq!(job.threads, 4);
    }

    #[test]
    fn cancel_token() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_token_clone_shares_state() {
        let token = CancelToken::new();
        let clone = token.clone();
        token.cancel();
        assert!(clone.is_cancelled());
    }
}
