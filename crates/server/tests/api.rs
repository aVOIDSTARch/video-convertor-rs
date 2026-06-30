//! End-to-end HTTP API tests against a real in-process server.

mod common;

use common::{make_wav, require_ffmpeg, TestServer};
use reqwest::multipart::{Form, Part};
use serde_json::Value;
use std::time::Duration;

async fn poll_until_terminal(client: &reqwest::Client, base: &str, job_id: &str) -> Value {
    for _ in 0..400 {
        let status: Value = client
            .get(format!("{base}/jobs/{job_id}/status"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let s = status.get("status").and_then(Value::as_str).unwrap_or("");
        if matches!(s, "completed" | "failed" | "cancelled") {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job did not finish in time");
}

#[tokio::test]
async fn health_is_open() {
    if !require_ffmpeg() {
        return;
    }
    let server = TestServer::start(None).await;
    let resp = reqwest::get(server.url("health")).await.unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn capabilities_and_presets() {
    if !require_ffmpeg() {
        return;
    }
    let server = TestServer::start(None).await;
    let caps: Value = reqwest::get(server.url("capabilities?kind=encoders"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(caps["encoders"].as_array().unwrap().len() > 20);

    let presets: Value = reqwest::get(server.url("presets")).await.unwrap().json().await.unwrap();
    assert!(presets.as_array().unwrap().len() >= 14);
}

#[tokio::test]
async fn convert_job_round_trip() {
    if !require_ffmpeg() {
        return;
    }
    let server = TestServer::start(None).await;
    let dir = common::tempdir();
    let wav = dir.path().join("in.wav");
    make_wav(&wav, 2.0);

    let client = reqwest::Client::new();
    let bytes = std::fs::read(&wav).unwrap();
    let form = Form::new()
        .text("request", r#"{"preset":"podcast-mp3"}"#)
        .part("file", Part::bytes(bytes).file_name("in.wav"));

    let submit: Value = client
        .post(server.url("convert"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = submit["job_id"].as_str().unwrap().to_string();
    assert_eq!(submit["status"], "queued");

    let status = poll_until_terminal(&client, &server.base, &job_id).await;
    assert_eq!(status["status"], "completed");

    // Download the result and confirm it is a non-trivial MP3.
    let result = client
        .get(server.url(&format!("jobs/{job_id}/result")))
        .send()
        .await
        .unwrap();
    assert!(result.status().is_success());
    let ct = result
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(ct.contains("audio"), "unexpected content-type {ct}");
    let body = result.bytes().await.unwrap();
    assert!(body.len() > 500, "mp3 too small: {}", body.len());

    // Delete cleans up.
    let del = client
        .delete(server.url(&format!("jobs/{job_id}")))
        .send()
        .await
        .unwrap();
    assert!(del.status().is_success());
    let after = client
        .get(server.url(&format!("jobs/{job_id}/status")))
        .send()
        .await
        .unwrap();
    assert_eq!(after.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn probe_job_returns_inline_json() {
    if !require_ffmpeg() {
        return;
    }
    let server = TestServer::start(None).await;
    let dir = common::tempdir();
    let wav = dir.path().join("in.wav");
    make_wav(&wav, 1.0);

    let client = reqwest::Client::new();
    let form = Form::new().part(
        "file",
        Part::bytes(std::fs::read(&wav).unwrap()).file_name("in.wav"),
    );
    let submit: Value = client
        .post(server.url("probe"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = submit["job_id"].as_str().unwrap().to_string();
    poll_until_terminal(&client, &server.base, &job_id).await;

    let result: Value = client
        .get(server.url(&format!("jobs/{job_id}/result")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(result.get("streams").is_some());
}

#[tokio::test]
async fn bad_request_rejected_synchronously() {
    if !require_ffmpeg() {
        return;
    }
    let server = TestServer::start(None).await;
    let client = reqwest::Client::new();
    // convert with no preset/format and no resolvable container → 400.
    let form = Form::new()
        .text("request", "{}")
        .part("file", Part::bytes(b"x".to_vec()).file_name("in.bin"));
    let resp = client.post(server.url("convert")).multipart(form).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn raw_disabled_job_fails() {
    if !require_ffmpeg() {
        return;
    }
    let server = TestServer::start(None).await;
    let dir = common::tempdir();
    let wav = dir.path().join("in.wav");
    make_wav(&wav, 1.0);

    let client = reqwest::Client::new();
    let form = Form::new()
        .text("request", r#"{"args":["-i","INPUT","OUTPUT"],"output_ext":"mp3"}"#)
        .part("file", Part::bytes(std::fs::read(&wav).unwrap()).file_name("in.wav"));
    let submit: Value = client
        .post(server.url("raw"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = submit["job_id"].as_str().unwrap().to_string();
    let status = poll_until_terminal(&client, &server.base, &job_id).await;
    assert_eq!(status["status"], "failed");
    assert!(status["error"].as_str().unwrap().contains("raw"));
}

#[tokio::test]
async fn token_auth_enforced() {
    if !require_ffmpeg() {
        return;
    }
    let server = TestServer::start(Some("s3cret")).await;
    let client = reqwest::Client::new();

    // Health is open even with a token configured.
    assert!(reqwest::get(server.url("health")).await.unwrap().status().is_success());

    // Presets without a token → 401.
    let no_auth = client.get(server.url("presets")).send().await.unwrap();
    assert_eq!(no_auth.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Wrong token → 401.
    let bad = client
        .get(server.url("presets"))
        .bearer_auth("nope")
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Correct token → 200.
    let ok = client
        .get(server.url("presets"))
        .bearer_auth("s3cret")
        .send()
        .await
        .unwrap();
    assert!(ok.status().is_success());
}

#[tokio::test]
async fn unknown_job_is_404() {
    if !require_ffmpeg() {
        return;
    }
    let server = TestServer::start(None).await;
    let resp = reqwest::get(server.url("jobs/00000000-0000-0000-0000-000000000000/status"))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}
