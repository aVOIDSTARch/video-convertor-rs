//! Progress reporting for long-running media operations.

use std::time::Duration;

/// A progress event emitted during transcoding.
#[derive(Debug, Clone)]
pub struct ProgressEvent {
    /// Current position in the source media.
    pub position: Duration,
    /// Total duration of the source media (if known).
    pub duration: Option<Duration>,
    /// Estimated percentage complete (0.0–100.0).
    pub percent: Option<f64>,
    /// Current encoding speed relative to realtime (e.g. 2.0 = 2x).
    pub speed: Option<f64>,
    /// Current output file size in bytes.
    pub output_size: Option<u64>,
}

impl ProgressEvent {
    /// Create a new progress event from position and optional duration.
    pub fn new(position: Duration, duration: Option<Duration>) -> Self {
        let percent = duration.map(|d| {
            if d.is_zero() {
                100.0
            } else {
                (position.as_secs_f64() / d.as_secs_f64() * 100.0).min(100.0)
            }
        });
        Self {
            position,
            duration,
            percent,
            speed: None,
            output_size: None,
        }
    }
}

/// Trait for receiving progress updates during transcoding.
///
/// Implement this trait to display progress bars, log events, or update UI.
pub trait ProgressHandler: Send {
    /// Called periodically with a progress update.
    fn on_progress(&mut self, event: &ProgressEvent);

    /// Called when the operation completes successfully.
    fn on_complete(&mut self) {
        // default: no-op
    }

    /// Called when the operation fails.
    fn on_error(&mut self, _error: &str) {
        // default: no-op
    }
}

/// A no-op progress handler that discards all events.
pub struct NoProgress;

impl ProgressHandler for NoProgress {
    fn on_progress(&mut self, _event: &ProgressEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_percent_calculation() {
        let e = ProgressEvent::new(
            Duration::from_secs(30),
            Some(Duration::from_secs(60)),
        );
        assert!((e.percent.unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn progress_percent_caps_at_100() {
        let e = ProgressEvent::new(
            Duration::from_secs(70),
            Some(Duration::from_secs(60)),
        );
        assert!((e.percent.unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn progress_no_duration() {
        let e = ProgressEvent::new(Duration::from_secs(10), None);
        assert!(e.percent.is_none());
    }

    #[test]
    fn progress_zero_duration() {
        let e = ProgressEvent::new(Duration::from_secs(0), Some(Duration::ZERO));
        assert!((e.percent.unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn no_progress_handler_compiles() {
        let mut h = NoProgress;
        let e = ProgressEvent::new(Duration::from_secs(1), None);
        h.on_progress(&e);
        h.on_complete();
        h.on_error("test");
    }
}
