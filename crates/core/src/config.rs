//! Application-level configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration for media conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Temporary directory for intermediate files. None = system default.
    pub temp_dir: Option<PathBuf>,
    /// Maximum number of concurrent transcode jobs (server mode).
    pub max_concurrent_jobs: usize,
    /// FFmpeg thread count per job. 0 = auto (ffmpeg decides).
    pub threads: u32,
    /// Whether to overwrite existing output files without prompting.
    pub overwrite: bool,
    /// Log level filter string (e.g. "info", "debug", "media_convertor_core=trace").
    pub log_level: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            temp_dir: None,
            max_concurrent_jobs: 4,
            threads: 0,
            overwrite: false,
            log_level: "info".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let c = AppConfig::default();
        assert!(c.temp_dir.is_none());
        assert_eq!(c.max_concurrent_jobs, 4);
        assert_eq!(c.threads, 0);
        assert!(!c.overwrite);
        assert_eq!(c.log_level, "info");
    }

    #[test]
    fn serde_roundtrip() {
        let c = AppConfig {
            temp_dir: Some(PathBuf::from("/tmp/media")),
            max_concurrent_jobs: 8,
            threads: 4,
            overwrite: true,
            log_level: "debug".to_string(),
        };
        let json = serde_json::to_string(&c).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.max_concurrent_jobs, 8);
        assert_eq!(parsed.threads, 4);
        assert!(parsed.overwrite);
    }

    #[test]
    fn deserialize_with_defaults() {
        let json = r#"{"overwrite": true}"#;
        let c: AppConfig = serde_json::from_str(json).unwrap();
        assert!(c.overwrite);
        assert_eq!(c.max_concurrent_jobs, 4); // default
        assert_eq!(c.log_level, "info"); // default
    }
}
