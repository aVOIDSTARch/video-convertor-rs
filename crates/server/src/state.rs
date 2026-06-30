//! Shared server state: the persistent queue plus the handler used for inline ops.

use media_convertor_core::api_queue::{ProgressReporter, Queue};
use media_convertor_core::{Config, FfmpegHandler};
use std::sync::Arc;

/// Shared application state, cloned into every request handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub queue: Arc<Queue>,
    /// The same handler the queue uses, kept for synchronous inline ops
    /// (capabilities/presets) that need no queueing.
    pub handler: Arc<FfmpegHandler>,
}

/// A no-op progress reporter for synchronous inline operations.
pub struct NoReporter;

impl ProgressReporter for NoReporter {
    fn report(&self, _progress: f64) {}
}
