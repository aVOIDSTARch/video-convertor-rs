//! [`FfmpegHandler`]: the [`QueueHandler`] that turns a [`UniversalRequest`] into an
//! [`Operation`] and executes it against the subprocess [`Engine`].
//!
//! This is the only place the reusable queue is wired to FFmpeg. Inline operations
//! (probe/capabilities/presets) return JSON; everything else produces an output file in
//! the managed output directory.

use crate::api_queue::{JobResult, ProgressReporter, QueueHandler, UniversalRequest};
use crate::container::Container;
use crate::ffmpeg::command;
use crate::ffmpeg::{CancelToken, Engine};
use crate::operation::{CapKind, Operation};
use crate::preset::Preset;
use crate::progress::{ProgressEvent, ProgressHandler};
use crate::security;
use serde_json::json;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

/// Executes universal requests against the FFmpeg engine.
pub struct FfmpegHandler {
    engine: Engine,
    output_dir: PathBuf,
    raw_enabled: bool,
}

impl FfmpegHandler {
    pub fn new(engine: Engine, output_dir: PathBuf, raw_enabled: bool) -> Self {
        Self {
            engine,
            output_dir,
            raw_enabled,
        }
    }

    /// Direct (non-queued) execution — used by the CLI's local synchronous paths and by
    /// tests. Mirrors [`QueueHandler::handle`] but takes a concrete progress reporter.
    pub fn execute(
        &self,
        job_id: Uuid,
        request: &UniversalRequest,
        reporter: &dyn ProgressReporter,
    ) -> crate::Result<JobResult> {
        let op = Operation::from_request(request)?;

        match op {
            Operation::Probe => self.run_probe(request),
            Operation::Capabilities { kind } => Ok(self.run_capabilities(kind)),
            Operation::Presets => Ok(run_presets()),
            Operation::Raw { args, output_ext } => {
                self.run_raw(job_id, request, args, output_ext, reporter)
            }
            Operation::Concat { container } => self.run_concat(job_id, request, container, reporter),
            other => self.run_file_op(job_id, request, other, reporter),
        }
    }

    // ── Inline operations ──────────────────────────────────────

    fn run_probe(&self, request: &UniversalRequest) -> crate::Result<JobResult> {
        let input = primary_input(request)?;
        security::ensure_safe_input_token(&input.to_string_lossy())?;
        let info = self.engine.probe(&input)?;
        Ok(JobResult {
            body: Some(serde_json::to_value(&info).unwrap_or(json!({}))),
            content_type: Some("application/json".to_string()),
            ..Default::default()
        })
    }

    fn run_capabilities(&self, kind: CapKind) -> JobResult {
        let caps = self.engine.capabilities();
        let body = match kind {
            CapKind::All => serde_json::to_value(caps).unwrap_or(json!({})),
            CapKind::Encoders => json!({ "encoders": caps.encoders }),
            CapKind::Decoders => json!({ "decoders": caps.decoders }),
            CapKind::Filters => json!({ "filters": caps.filters }),
            CapKind::Muxers => json!({ "muxers": caps.muxers }),
            CapKind::Demuxers => json!({ "demuxers": caps.demuxers }),
            CapKind::PixFmts => json!({ "pix_fmts": caps.pix_fmts }),
            CapKind::Protocols => json!({
                "protocols_input": caps.protocols_input,
                "protocols_output": caps.protocols_output,
            }),
        };
        JobResult {
            body: Some(body),
            content_type: Some("application/json".to_string()),
            ..Default::default()
        }
    }

    // ── File-producing operations ──────────────────────────────

    fn run_file_op(
        &self,
        job_id: Uuid,
        request: &UniversalRequest,
        op: Operation,
        reporter: &dyn ProgressReporter,
    ) -> crate::Result<JobResult> {
        let input = primary_input(request)?;
        security::ensure_safe_input_token(&input.to_string_lossy())?;

        let input_ext = input.extension().and_then(|e| e.to_str());
        let ext = op.output_extension(input_ext);
        let output = self.output_path(job_id, &ext)?;

        let (args, container_for_mime): (Vec<OsString>, Option<Container>) = match &op {
            Operation::Convert { format, trim } => (
                command::transcode_args(&input, &output, format, trim),
                Some(format.container),
            ),
            Operation::ExtractAudio { codec, container } => (
                command::extract_audio_args(&input, &output, codec.as_deref()),
                Some(*container),
            ),
            Operation::Thumbnail { time, width } => {
                (command::thumbnail_args(&input, &output, *time, *width), None)
            }
            Operation::Filter { graph, container } => {
                (command::filter_args(&input, &output, graph), *container)
            }
            _ => unreachable!("non-file op routed to run_file_op"),
        };

        let total = self.probe_duration(&input);
        self.run_engine(&args, total, reporter)?;
        finalize_output(output, container_for_mime, input_filename(request))
    }

    fn run_concat(
        &self,
        job_id: Uuid,
        request: &UniversalRequest,
        container: Container,
        reporter: &dyn ProgressReporter,
    ) -> crate::Result<JobResult> {
        if request.attachments.len() < 2 {
            return Err(crate::MediaError::other("concat requires at least 2 inputs"));
        }
        for att in &request.attachments {
            security::ensure_safe_input_token(&att.path.to_string_lossy())?;
        }

        let output = self.output_path(job_id, container.extension())?;
        let list_path = self.output_dir.join(format!("{job_id}.concat.txt"));
        let mut list = String::new();
        for att in &request.attachments {
            // concat demuxer list format; single-quote and escape embedded quotes.
            let p = att.path.to_string_lossy().replace('\'', "'\\''");
            list.push_str(&format!("file '{p}'\n"));
        }
        std::fs::write(&list_path, list)?;

        let args = command::concat_args(&list_path, &output);
        let result = self.run_engine(&args, None, reporter);
        let _ = std::fs::remove_file(&list_path);
        result?;

        finalize_output(output, Some(container), None)
    }

    fn run_raw(
        &self,
        job_id: Uuid,
        request: &UniversalRequest,
        args: Vec<String>,
        output_ext: String,
        reporter: &dyn ProgressReporter,
    ) -> crate::Result<JobResult> {
        if !self.raw_enabled {
            return Err(crate::MediaError::security(
                "raw passthrough is disabled on this server".to_string(),
            ));
        }
        let input = primary_input(request)?;
        security::ensure_safe_input_token(&input.to_string_lossy())?;

        // Validate every token except the INPUT/OUTPUT placeholders.
        let to_check: Vec<String> = args
            .iter()
            .filter(|a| a.as_str() != "INPUT" && a.as_str() != "OUTPUT")
            .cloned()
            .collect();
        security::validate_raw_args(&to_check)?;

        let output = self.output_path(job_id, output_ext.trim_start_matches('.'))?;
        let built: Vec<OsString> = args
            .iter()
            .map(|a| match a.as_str() {
                "INPUT" => input.clone().into_os_string(),
                "OUTPUT" => output.clone().into_os_string(),
                other => OsString::from(other),
            })
            .collect();

        let total = self.probe_duration(&input);
        self.run_engine(&built, total, reporter)?;
        finalize_output(output, None, None)
    }

    // ── Helpers ────────────────────────────────────────────────

    fn run_engine(
        &self,
        args: &[OsString],
        total: Option<Duration>,
        reporter: &dyn ProgressReporter,
    ) -> crate::Result<()> {
        let mut adapter = ReporterProgress { reporter };
        let cancel = CancelToken::new();
        self.engine.run(args, total, &mut adapter, Some(&cancel))
    }

    fn probe_duration(&self, input: &Path) -> Option<Duration> {
        self.engine.probe(input).ok().and_then(|info| info.duration)
    }

    fn output_path(&self, job_id: Uuid, ext: &str) -> crate::Result<PathBuf> {
        let name = format!("{job_id}.{ext}");
        security::confine_path(&self.output_dir, Path::new(&name))
    }
}

impl QueueHandler for FfmpegHandler {
    fn handle(
        &self,
        job_id: Uuid,
        request: &UniversalRequest,
        reporter: &dyn ProgressReporter,
    ) -> std::result::Result<JobResult, String> {
        self.execute(job_id, request, reporter).map_err(|e| e.to_string())
    }
}

/// List presets as an inline JSON result.
fn run_presets() -> JobResult {
    JobResult {
        body: Some(serde_json::to_value(Preset::all()).unwrap_or(json!([]))),
        content_type: Some("application/json".to_string()),
        ..Default::default()
    }
}

/// The first attachment's path, or an error if none is present.
fn primary_input(request: &UniversalRequest) -> crate::Result<PathBuf> {
    request
        .attachments
        .first()
        .map(|a| a.path.clone())
        .ok_or_else(|| crate::MediaError::other("operation requires an input file"))
}

fn input_filename(request: &UniversalRequest) -> Option<String> {
    request.attachments.first().map(|a| a.filename.clone())
}

/// Compute size/content-type for a produced output file.
fn finalize_output(
    output: PathBuf,
    container: Option<Container>,
    original_name: Option<String>,
) -> crate::Result<JobResult> {
    let size = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    let content_type = container
        .map(|c| c.mime_type().to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Suggest a friendly download name based on the original input stem.
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("out");
    let stem = original_name
        .as_deref()
        .and_then(|n| Path::new(n).file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .unwrap_or_else(|| "output".to_string());
    let output_name = format!("{stem}.{ext}");

    Ok(JobResult {
        output_path: Some(output),
        output_name: Some(output_name),
        content_type: Some(content_type),
        body: None,
        size,
    })
}

/// Adapts the queue's [`ProgressReporter`] to the engine's [`ProgressHandler`].
struct ReporterProgress<'a> {
    reporter: &'a dyn ProgressReporter,
}

impl ProgressHandler for ReporterProgress<'_> {
    fn on_progress(&mut self, event: &ProgressEvent) {
        if let Some(pct) = event.percent {
            self.reporter.report(pct);
        }
    }
}
