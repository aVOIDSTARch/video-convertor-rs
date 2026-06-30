//! Local (in-process) execution: dispatch a `UniversalRequest` through the shared
//! `FfmpegHandler` — the exact same code path the server's queue workers use.

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use media_convertor_core::api_queue::{Attachment, JobResult, ProgressReporter, UniversalRequest};
use media_convertor_core::{Config, Engine, FfmpegHandler};
use std::path::Path;
use uuid::Uuid;

/// A progress reporter backed by an indicatif bar (or nothing, when quiet).
pub struct BarReporter {
    bar: Option<ProgressBar>,
}

impl BarReporter {
    pub fn new(quiet: bool, label: &str) -> Self {
        if quiet {
            return Self { bar: None };
        }
        let bar = ProgressBar::new(100);
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% {msg}",
            )
            .unwrap()
            .progress_chars("#>-"),
        );
        bar.set_message(label.to_string());
        Self { bar: Some(bar) }
    }

    pub fn finish(&self) {
        if let Some(bar) = &self.bar {
            bar.finish_with_message("done");
        }
    }
}

impl ProgressReporter for BarReporter {
    fn report(&self, progress: f64) {
        if let Some(bar) = &self.bar {
            bar.set_position(progress as u64);
        }
    }
}

/// Build a handler bound to the given config (locates ffmpeg, discovers capabilities).
pub fn build_handler(config: &Config, raw_enabled: bool) -> Result<FfmpegHandler> {
    config.ensure_dirs().context("creating work directories")?;
    let engine = Engine::new(config).context("initializing the ffmpeg engine")?;
    Ok(FfmpegHandler::new(engine, config.output_dir(), raw_enabled))
}

/// Make an [`Attachment`] that points directly at a user-provided local file (no copy).
pub fn local_attachment(field: &str, path: &Path) -> Result<Attachment> {
    if !path.exists() {
        anyhow::bail!("input file not found: {}", path.display());
    }
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input")
        .to_string();
    Ok(Attachment {
        field: field.to_string(),
        filename,
        path: path.to_path_buf(),
    })
}

/// Run a request locally and, for file-producing ops, move the result to `output`.
/// For inline ops (probe/capabilities/presets) the JSON body is returned for printing.
pub fn run(
    handler: &FfmpegHandler,
    request: UniversalRequest,
    output: Option<&Path>,
    quiet: bool,
    label: &str,
) -> Result<JobResult> {
    let reporter = BarReporter::new(quiet, label);
    let result = handler
        .execute(Uuid::new_v4(), &request, &reporter)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    reporter.finish();

    if let (Some(produced), Some(dest)) = (result.output_path.as_ref(), output) {
        move_file(produced, dest)
            .with_context(|| format!("writing output to {}", dest.display()))?;
    }
    Ok(result)
}

/// Move a file, falling back to copy+remove across filesystem boundaries.
fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            let _ = std::fs::remove_file(from);
            Ok(())
        }
    }
}
