//! A self-contained, reusable, persistent job queue.
//!
//! Work items are **universal HTTP-shaped request objects** ([`UniversalRequest`]):
//! a method, path, query, headers, JSON body, and file attachments. Both the CLI and the
//! HTTP API build the same shape and enqueue it; a bounded pool of worker threads drains
//! the queue and dispatches each request to a [`QueueHandler`].
//!
//! This module has **no FFmpeg dependency** — it only knows about requests, jobs, and a
//! handler trait — so it can be lifted into any project that needs a durable request
//! queue. The FFmpeg behavior is provided by [`crate::dispatch::FfmpegHandler`].
//!
//! Durability: every job is persisted to `jobs/{id}.json` on each transition, and the
//! queue can be recovered after a restart (in-flight jobs are re-queued, not lost).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// HTTP-ish method for a universal request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

/// A file attached to a request, already stored on disk in a managed directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Form field name (e.g. `"file"`, `"input"`, `"input2"`).
    pub field: String,
    /// Sanitized original filename.
    pub filename: String,
    /// Absolute path of the stored file.
    pub path: PathBuf,
}

/// The universal, serializable request object that the queue stores and dispatches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalRequest {
    pub method: Method,
    pub path: String,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Value,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl UniversalRequest {
    /// A GET request to `path`.
    pub fn get(path: impl Into<String>) -> Self {
        Self::new(Method::Get, path)
    }

    /// A POST request to `path`.
    pub fn post(path: impl Into<String>) -> Self {
        Self::new(Method::Post, path)
    }

    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: BTreeMap::new(),
            headers: BTreeMap::new(),
            body: Value::Null,
            attachments: Vec::new(),
        }
    }

    pub fn with_body(mut self, body: Value) -> Self {
        self.body = body;
        self
    }

    pub fn with_query(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.insert(key.into(), value.into());
        self
    }

    pub fn with_attachment(mut self, attachment: Attachment) -> Self {
        self.attachments.push(attachment);
        self
    }

    /// Look up a string field in the JSON body.
    pub fn body_str(&self, key: &str) -> Option<&str> {
        self.body.get(key).and_then(Value::as_str)
    }

    /// Look up a float field in the JSON body (accepting numeric strings).
    pub fn body_f64(&self, key: &str) -> Option<f64> {
        self.body
            .get(key)
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
    }

    /// Look up an unsigned-integer field in the JSON body (accepting numeric strings).
    pub fn body_u64(&self, key: &str) -> Option<u64> {
        self.body
            .get(key)
            .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
    }
}

/// Lifecycle status of a queued job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled)
    }
}

/// The result a handler produces for a completed job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobResult {
    /// Output file path (for file-producing operations).
    pub output_path: Option<PathBuf>,
    /// Suggested download filename.
    pub output_name: Option<String>,
    /// MIME type of the output (file) or `application/json` (inline).
    pub content_type: Option<String>,
    /// Inline JSON result (probe/capabilities/presets).
    pub body: Option<Value>,
    /// Output size in bytes.
    pub size: u64,
}

/// A persisted job: the request plus its evolving status/result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub request: UniversalRequest,
    pub status: JobStatus,
    pub progress: f64,
    pub error: Option<String>,
    pub result: Option<JobResult>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Job {
    fn new(request: UniversalRequest) -> Self {
        let now = now_secs();
        Self {
            id: Uuid::new_v4(),
            request,
            status: JobStatus::Queued,
            progress: 0.0,
            error: None,
            result: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Receives progress updates from a handler while it processes a job.
pub trait ProgressReporter: Send + Sync {
    /// Report progress in the range `0.0..=100.0`.
    fn report(&self, progress: f64);
}

/// Handles a dequeued request. Implementations execute the work and return a result.
pub trait QueueHandler: Send + Sync {
    fn handle(
        &self,
        job_id: Uuid,
        request: &UniversalRequest,
        reporter: &dyn ProgressReporter,
    ) -> std::result::Result<JobResult, String>;
}

/// Shared, persisted job store.
#[derive(Clone)]
struct JobStore {
    jobs: Arc<Mutex<HashMap<Uuid, Job>>>,
    jobs_dir: PathBuf,
}

impl JobStore {
    fn new(jobs_dir: PathBuf) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            jobs_dir,
        }
    }

    fn persist(&self, job: &Job) {
        let path = self.jobs_dir.join(format!("{}.json", job.id));
        match serde_json::to_vec_pretty(job) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, bytes) {
                    tracing::warn!("failed to persist job {}: {e}", job.id);
                }
            }
            Err(e) => tracing::warn!("failed to serialize job {}: {e}", job.id),
        }
    }

    fn insert(&self, job: Job) {
        self.persist(&job);
        self.jobs.lock().unwrap().insert(job.id, job);
    }

    fn get(&self, id: &Uuid) -> Option<Job> {
        self.jobs.lock().unwrap().get(id).cloned()
    }

    fn list(&self) -> Vec<Job> {
        self.jobs.lock().unwrap().values().cloned().collect()
    }

    /// Apply a mutation to a job, persist it, and return the updated copy.
    fn update<F: FnOnce(&mut Job)>(&self, id: &Uuid, f: F) -> Option<Job> {
        let mut guard = self.jobs.lock().unwrap();
        let job = guard.get_mut(id)?;
        f(job);
        job.updated_at = now_secs();
        let snapshot = job.clone();
        drop(guard);
        self.persist(&snapshot);
        Some(snapshot)
    }

    fn remove(&self, id: &Uuid) -> Option<Job> {
        let removed = self.jobs.lock().unwrap().remove(id);
        if removed.is_some() {
            let _ = std::fs::remove_file(self.jobs_dir.join(format!("{id}.json")));
        }
        removed
    }
}

/// The bounded, persistent queue with its worker pool.
pub struct Queue {
    store: JobStore,
    tx: Sender<Uuid>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl Queue {
    /// Create a queue with `workers` worker threads, persisting jobs under `jobs_dir`.
    ///
    /// Recovers any persisted jobs: terminal jobs are kept for status/result queries;
    /// non-terminal jobs (`Queued`/`Running`) are reset to `Queued` and re-enqueued.
    pub fn new(jobs_dir: PathBuf, workers: usize, handler: Arc<dyn QueueHandler>) -> std::io::Result<Self> {
        std::fs::create_dir_all(&jobs_dir)?;
        let store = JobStore::new(jobs_dir.clone());

        let (tx, rx) = mpsc::channel::<Uuid>();
        let rx = Arc::new(Mutex::new(rx));

        let workers = workers.max(1);
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            handles.push(spawn_worker(store.clone(), Arc::clone(&rx), Arc::clone(&handler)));
        }

        let queue = Self {
            store,
            tx,
            workers: handles,
        };

        queue.recover();
        Ok(queue)
    }

    /// Submit a request, returning the new job id.
    pub fn submit(&self, request: UniversalRequest) -> Uuid {
        let job = Job::new(request);
        let id = job.id;
        self.store.insert(job);
        // Send can only fail if all workers are gone, which never happens while alive.
        let _ = self.tx.send(id);
        id
    }

    /// Fetch a job snapshot.
    pub fn get(&self, id: &Uuid) -> Option<Job> {
        self.store.get(id)
    }

    /// All jobs.
    pub fn list(&self) -> Vec<Job> {
        self.store.list()
    }

    /// Remove a job (and its persisted record). Returns the removed job, if any.
    pub fn remove(&self, id: &Uuid) -> Option<Job> {
        self.store.remove(id)
    }

    /// Number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    fn recover(&self) {
        let dir = match std::fs::read_dir(&self.store.jobs_dir) {
            Ok(d) => d,
            Err(_) => return,
        };
        let mut requeue = Vec::new();
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let mut job: Job = match serde_json::from_slice(&bytes) {
                Ok(j) => j,
                Err(e) => {
                    tracing::warn!("skipping unreadable job file {}: {e}", path.display());
                    continue;
                }
            };
            // Already in memory? Skip.
            if self.store.get(&job.id).is_some() {
                continue;
            }
            if !job.status.is_terminal() {
                // In-flight at shutdown: reset and re-enqueue so nothing is lost.
                job.status = JobStatus::Queued;
                job.progress = 0.0;
                requeue.push(job.id);
            }
            self.store.jobs.lock().unwrap().insert(job.id, job);
        }
        for id in requeue {
            if let Some(job) = self.store.get(&id) {
                self.store.persist(&job);
            }
            let _ = self.tx.send(id);
        }
    }
}

fn spawn_worker(
    store: JobStore,
    rx: Arc<Mutex<Receiver<Uuid>>>,
    handler: Arc<dyn QueueHandler>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        // Lock only to receive; release before processing so workers run concurrently.
        let id = {
            let guard = rx.lock().unwrap();
            guard.recv()
        };
        let id = match id {
            Ok(id) => id,
            Err(_) => break, // sender dropped: shut down
        };

        let request = match store.get(&id) {
            Some(job) => job.request.clone(),
            None => continue, // removed before processing
        };

        store.update(&id, |j| {
            j.status = JobStatus::Running;
            j.progress = 0.0;
        });

        let reporter = StoreReporter {
            store: store.clone(),
            id,
            last: Mutex::new(0.0),
        };

        match handler.handle(id, &request, &reporter) {
            Ok(result) => {
                store.update(&id, |j| {
                    j.status = JobStatus::Completed;
                    j.progress = 100.0;
                    j.result = Some(result);
                });
            }
            Err(err) => {
                store.update(&id, |j| {
                    j.status = JobStatus::Failed;
                    j.error = Some(err);
                });
            }
        }
    })
}

/// A [`ProgressReporter`] that writes progress back into the store (throttled persist).
struct StoreReporter {
    store: JobStore,
    id: Uuid,
    last: Mutex<f64>,
}

impl ProgressReporter for StoreReporter {
    fn report(&self, progress: f64) {
        let progress = progress.clamp(0.0, 100.0);
        let mut last = self.last.lock().unwrap();
        // Persist only on meaningful change to limit disk churn.
        if (progress - *last).abs() < 0.5 && progress < 100.0 {
            return;
        }
        *last = progress;
        drop(last);
        self.store.update(&self.id, |j| j.progress = progress);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct CountingHandler {
        seen: Arc<AtomicUsize>,
    }

    impl QueueHandler for CountingHandler {
        fn handle(
            &self,
            _id: Uuid,
            req: &UniversalRequest,
            reporter: &dyn ProgressReporter,
        ) -> std::result::Result<JobResult, String> {
            reporter.report(50.0);
            self.seen.fetch_add(1, Ordering::SeqCst);
            if req.path.contains("fail") {
                return Err("boom".to_string());
            }
            Ok(JobResult {
                body: Some(serde_json::json!({"ok": true})),
                content_type: Some("application/json".to_string()),
                ..Default::default()
            })
        }
    }

    fn wait_for<F: Fn() -> bool>(f: F) {
        for _ in 0..200 {
            if f() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("condition not met in time");
    }

    #[test]
    fn processes_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(CountingHandler { seen: seen.clone() });
        let queue = Queue::new(dir.path().to_path_buf(), 2, handler).unwrap();

        let id = queue.submit(UniversalRequest::get("test"));
        wait_for(|| queue.get(&id).map(|j| j.status.is_terminal()).unwrap_or(false));

        let job = queue.get(&id).unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert!(dir.path().join(format!("{id}.json")).exists());
    }

    #[test]
    fn failure_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let handler = Arc::new(CountingHandler {
            seen: Arc::new(AtomicUsize::new(0)),
        });
        let queue = Queue::new(dir.path().to_path_buf(), 1, handler).unwrap();
        let id = queue.submit(UniversalRequest::get("fail"));
        wait_for(|| queue.get(&id).map(|j| j.status.is_terminal()).unwrap_or(false));
        let job = queue.get(&id).unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error.as_deref(), Some("boom"));
    }

    #[test]
    fn recovers_persisted_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let jobs_dir = dir.path().to_path_buf();

        // Write a stuck "running" job file directly, simulating a crash mid-flight.
        let mut job = Job::new(UniversalRequest::get("test"));
        job.status = JobStatus::Running;
        std::fs::write(
            jobs_dir.join(format!("{}.json", job.id)),
            serde_json::to_vec(&job).unwrap(),
        )
        .unwrap();

        let handler = Arc::new(CountingHandler {
            seen: Arc::new(AtomicUsize::new(0)),
        });
        let queue = Queue::new(jobs_dir, 1, handler).unwrap();
        // Recovered + re-enqueued → eventually completes.
        wait_for(|| queue.get(&job.id).map(|j| j.status == JobStatus::Completed).unwrap_or(false));
    }
}
