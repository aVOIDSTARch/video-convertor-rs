//! Integration tests for the FfmpegHandler dispatch layer and the end-to-end queue.

mod common;

use common::*;
use media_convertor_core::api_queue::{
    Attachment, Method, ProgressReporter, Queue, QueueHandler, UniversalRequest,
};
use media_convertor_core::{Config, Engine, FfmpegHandler};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

struct Noop;
impl ProgressReporter for Noop {
    fn report(&self, _p: f64) {}
}

fn handler(config: &Config, raw: bool) -> FfmpegHandler {
    config.ensure_dirs().unwrap();
    let engine = Engine::new(config).expect("engine");
    FfmpegHandler::new(engine, config.output_dir(), raw)
}

fn attach(field: &str, path: &std::path::Path) -> Attachment {
    Attachment {
        field: field.to_string(),
        filename: path.file_name().unwrap().to_string_lossy().into_owned(),
        path: path.to_path_buf(),
    }
}

#[test]
fn dispatch_convert_produces_output() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let h = handler(&cfg, false);
    let wav = child(dir.path(), "in.wav");
    make_wav(&wav, 2.0);

    let req = UniversalRequest::post("convert")
        .with_body(json!({"preset": "podcast-mp3"}))
        .with_attachment(attach("file", &wav));
    let result = h.execute(Uuid::new_v4(), &req, &Noop).expect("execute");
    let out = result.output_path.expect("output path");
    assert!(out.exists());
    assert_eq!(stream_codecs(&out), vec!["mp3"]);
    assert!(out.starts_with(cfg.output_dir()));
}

#[test]
fn dispatch_probe_returns_json() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let h = handler(&cfg, false);
    let mp4 = child(dir.path(), "in.mp4");
    make_mp4(&mp4, 1.0);

    let req = UniversalRequest::post("probe").with_attachment(attach("file", &mp4));
    let result = h.execute(Uuid::new_v4(), &req, &Noop).unwrap();
    let body = result.body.expect("inline body");
    assert!(body.get("streams").is_some());
    assert!(body.get("format_name").is_some());
}

#[test]
fn dispatch_capabilities_and_presets() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let h = handler(&cfg, false);

    let caps = h
        .execute(
            Uuid::new_v4(),
            &UniversalRequest::new(Method::Get, "capabilities").with_query("kind", "encoders"),
            &Noop,
        )
        .unwrap();
    assert!(caps.body.unwrap().get("encoders").is_some());

    let presets = h
        .execute(Uuid::new_v4(), &UniversalRequest::new(Method::Get, "presets"), &Noop)
        .unwrap();
    assert!(presets.body.unwrap().as_array().unwrap().len() >= 14);
}

#[test]
fn dispatch_concat_requires_two_inputs() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let h = handler(&cfg, false);
    let mp4 = child(dir.path(), "in.mp4");
    make_mp4(&mp4, 1.0);

    let one = UniversalRequest::post("concat")
        .with_body(json!({"format": "mkv"}))
        .with_attachment(attach("file", &mp4));
    assert!(h.execute(Uuid::new_v4(), &one, &Noop).is_err());
}

#[test]
fn dispatch_raw_gated_off_by_default() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let h = handler(&cfg, false); // raw disabled
    let wav = child(dir.path(), "in.wav");
    make_wav(&wav, 1.0);

    let req = UniversalRequest::post("raw")
        .with_body(json!({"args": ["-i", "INPUT", "OUTPUT"], "output_ext": "mp3"}))
        .with_attachment(attach("file", &wav));
    let err = h.execute(Uuid::new_v4(), &req, &Noop).unwrap_err();
    assert!(err.to_string().contains("raw"), "got {err}");
}

#[test]
fn dispatch_raw_enabled_runs() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let h = handler(&cfg, true); // raw enabled
    let wav = child(dir.path(), "in.wav");
    make_wav(&wav, 1.0);

    let req = UniversalRequest::post("raw")
        .with_body(json!({"args": ["-i", "INPUT", "-c:a", "libmp3lame", "OUTPUT"], "output_ext": "mp3"}))
        .with_attachment(attach("file", &wav));
    let result = h.execute(Uuid::new_v4(), &req, &Noop).expect("raw run");
    assert!(result.output_path.unwrap().exists());
}

#[test]
fn dispatch_rejects_protocol_input() {
    // No ffmpeg needed: security check happens before any subprocess.
    let dir = tempdir();
    let cfg = test_config(dir.path());
    cfg.ensure_dirs().unwrap();
    // Build handler only if ffmpeg present; otherwise emulate by skipping.
    if !require_ffmpeg() {
        return;
    }
    let h = handler(&cfg, false);

    let req = UniversalRequest::post("probe").with_attachment(Attachment {
        field: "file".into(),
        filename: "x".into(),
        path: "http://evil/x".into(),
    });
    let err = h.execute(Uuid::new_v4(), &req, &Noop).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("protocol"), "got {err}");
}

// ── End-to-end queue tests ─────────────────────────────────────

fn wait_until<F: Fn() -> bool>(f: F) -> bool {
    for _ in 0..400 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

#[test]
fn queue_processes_convert_end_to_end() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let h: Arc<dyn QueueHandler> = Arc::new(handler(&cfg, false));
    let queue = Queue::new(cfg.jobs_dir(), 2, h).unwrap();

    let wav = child(dir.path(), "in.wav");
    make_wav(&wav, 2.0);

    let req = UniversalRequest::post("convert")
        .with_body(json!({"preset": "podcast-mp3"}))
        .with_attachment(attach("file", &wav));
    let id = queue.submit(req);

    assert!(wait_until(|| queue
        .get(&id)
        .map(|j| j.status.is_terminal())
        .unwrap_or(false)));
    let job = queue.get(&id).unwrap();
    assert_eq!(job.status, media_convertor_core::JobStatus::Completed);
    assert!(job.result.unwrap().output_path.unwrap().exists());
}

#[test]
fn queue_bounded_concurrency() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let h: Arc<dyn QueueHandler> = Arc::new(handler(&cfg, false));
    let queue = Queue::new(cfg.jobs_dir(), 2, h).unwrap();
    assert_eq!(queue.worker_count(), 2);

    let wav = child(dir.path(), "in.wav");
    make_wav(&wav, 1.0);

    let ids: Vec<_> = (0..5)
        .map(|_| {
            queue.submit(
                UniversalRequest::post("convert")
                    .with_body(json!({"preset": "podcast-mp3"}))
                    .with_attachment(attach("file", &wav)),
            )
        })
        .collect();

    for id in &ids {
        assert!(wait_until(|| queue
            .get(id)
            .map(|j| j.status.is_terminal())
            .unwrap_or(false)));
        assert_eq!(queue.get(id).unwrap().status, media_convertor_core::JobStatus::Completed);
    }
}

#[test]
fn queue_recovers_after_restart() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let cfg = test_config(dir.path());
    let wav = child(dir.path(), "in.wav");
    make_wav(&wav, 1.0);

    // First queue: process a job to completion, then drop the queue.
    let id = {
        let h: Arc<dyn QueueHandler> = Arc::new(handler(&cfg, false));
        let queue = Queue::new(cfg.jobs_dir(), 1, h).unwrap();
        let id = queue.submit(
            UniversalRequest::post("convert")
                .with_body(json!({"preset": "podcast-mp3"}))
                .with_attachment(attach("file", &wav)),
        );
        assert!(wait_until(|| queue
            .get(&id)
            .map(|j| j.status.is_terminal())
            .unwrap_or(false)));
        id
    };

    // Second queue over the same jobs dir recovers the persisted job.
    let h2: Arc<dyn QueueHandler> = Arc::new(handler(&cfg, false));
    let queue2 = Queue::new(cfg.jobs_dir(), 1, h2).unwrap();
    let recovered = queue2.get(&id).expect("job recovered after restart");
    assert_eq!(recovered.status, media_convertor_core::JobStatus::Completed);
}
