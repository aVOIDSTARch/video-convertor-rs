//! Run an ffmpeg child process with progress, timeout, and cancellation.

use super::CancelToken;
use crate::error::MediaError;
use crate::progress::{ProgressEvent, ProgressHandler};
use crate::Result;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One parsed `-progress` update.
#[derive(Debug, Default, Clone)]
struct ProgressUpdate {
    out_time_us: Option<u64>,
    total_size: Option<u64>,
    speed: Option<f64>,
    done: bool,
}

/// Run ffmpeg with the given (fully-built) argument vector.
///
/// `total` is the source duration used to compute a percentage. Progress is read from the
/// child's stdout (`-progress pipe:1`); stderr is captured for error reporting. The child
/// is killed if `cancel` fires or `timeout` elapses.
pub fn run(
    ffmpeg: &Path,
    args: &[OsString],
    total: Option<Duration>,
    progress: &mut dyn ProgressHandler,
    cancel: Option<&CancelToken>,
    timeout: Option<Duration>,
) -> Result<()> {
    let mut child = Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| MediaError::Ffmpeg(format!("failed to spawn ffmpeg: {e}")))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // Drain stderr into a bounded tail buffer on a background thread.
    let stderr_tail: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let stderr_tail_w = Arc::clone(&stderr_tail);
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut reader = stderr;
        let _ = reader.read_to_end(&mut buf);
        let text = String::from_utf8_lossy(&buf);
        let tail: String = text.chars().rev().take(4000).collect::<String>().chars().rev().collect();
        if let Ok(mut g) = stderr_tail_w.lock() {
            *g = tail;
        }
    });

    // Parse `-progress` blocks on a background thread, forwarding updates over a channel.
    let (tx, rx) = mpsc::channel::<ProgressUpdate>();
    let progress_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut acc = ProgressUpdate::default();
        for line in reader.lines().map_while(std::result::Result::ok) {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "out_time_us" | "out_time_ms" => {
                        // out_time_us is microseconds; out_time_ms is *also* microseconds
                        // in ffmpeg's confusing naming. Treat both as microseconds.
                        if let Ok(v) = value.trim().parse::<u64>() {
                            acc.out_time_us = Some(v);
                        }
                    }
                    "total_size" => acc.total_size = value.trim().parse::<u64>().ok(),
                    "speed" => {
                        let s = value.trim().trim_end_matches('x');
                        acc.speed = s.parse::<f64>().ok();
                    }
                    "progress" => {
                        acc.done = value.trim() == "end";
                        let _ = tx.send(std::mem::take(&mut acc));
                    }
                    _ => {}
                }
            }
        }
    });

    let start = Instant::now();
    let poll = Duration::from_millis(200);
    let mut killed_reason: Option<MediaError> = None;

    let status = loop {
        // Apply any pending progress updates.
        while let Ok(update) = rx.try_recv() {
            apply_update(progress, &update, total);
        }

        if let Some(c) = cancel {
            if c.is_cancelled() && killed_reason.is_none() {
                let _ = child.kill();
                killed_reason = Some(MediaError::Cancelled);
            }
        }
        if let Some(limit) = timeout {
            if start.elapsed() > limit && killed_reason.is_none() {
                let _ = child.kill();
                killed_reason = Some(MediaError::Timeout(limit));
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                // Block briefly on the next progress update so we stay responsive without
                // busy-spinning.
                if let Ok(update) = rx.recv_timeout(poll) {
                    apply_update(progress, &update, total);
                }
            }
            Err(e) => {
                let _ = child.kill();
                return Err(MediaError::Ffmpeg(format!("error waiting on ffmpeg: {e}")));
            }
        }
    };

    let _ = progress_thread.join();
    let _ = stderr_thread.join();

    if let Some(reason) = killed_reason {
        progress.on_error(&reason.to_string());
        return Err(reason);
    }

    if status.success() {
        progress.on_complete();
        Ok(())
    } else {
        let tail = stderr_tail.lock().map(|g| g.clone()).unwrap_or_default();
        let msg = if tail.trim().is_empty() {
            format!("ffmpeg exited with {status}")
        } else {
            format!("ffmpeg exited with {status}: {}", tail.trim())
        };
        progress.on_error(&msg);
        Err(MediaError::Ffmpeg(msg))
    }
}

fn apply_update(progress: &mut dyn ProgressHandler, update: &ProgressUpdate, total: Option<Duration>) {
    let position = Duration::from_micros(update.out_time_us.unwrap_or(0));
    let mut event = ProgressEvent::new(position, total);
    event.speed = update.speed;
    event.output_size = update.total_size;
    progress.on_progress(&event);
}
