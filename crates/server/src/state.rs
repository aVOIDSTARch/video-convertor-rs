//! Shared application state and job tracking.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub jobs: Arc<RwLock<HashMap<Uuid, Job>>>,
    pub upload_dir: PathBuf,
    pub output_dir: PathBuf,
}

impl AppState {
    pub fn new(upload_dir: PathBuf, output_dir: PathBuf) -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            upload_dir,
            output_dir,
        }
    }

    pub fn insert_job(&self, job: Job) -> Uuid {
        let id = job.id;
        self.jobs.write().unwrap().insert(id, job);
        id
    }

    pub fn get_job(&self, id: &Uuid) -> Option<Job> {
        self.jobs.read().unwrap().get(id).cloned()
    }

    pub fn update_status(&self, id: &Uuid, status: JobStatus) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.status = status;
        }
    }
}

/// A conversion job tracked by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub status: JobStatus,
    pub input_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub preset: Option<String>,
    pub error: Option<String>,
    pub progress: f64,
}

impl Job {
    pub fn new(input_path: PathBuf) -> Self {
        Self {
            id: Uuid::new_v4(),
            status: JobStatus::Queued,
            input_path,
            output_path: None,
            preset: None,
            error: None,
            progress: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}
