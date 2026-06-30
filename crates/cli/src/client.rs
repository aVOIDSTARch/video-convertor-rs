//! Remote mode: drive the HTTP API. The CLI builds the same request shape it uses
//! locally, submits it, polls the job, and downloads the result.

use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::multipart::Form;
use reqwest::blocking::Client as HttpClient;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// A thin client for the media-convertor HTTP API.
pub struct Client {
    base: String,
    token: Option<String>,
    http: HttpClient,
}

impl Client {
    pub fn new(base: &str, token: Option<String>) -> Result<Self> {
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(3600))
            .build()
            .context("building HTTP client")?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            http,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/{}", self.base, path.trim_start_matches('/'))
    }

    fn auth(&self, req: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        match &self.token {
            Some(t) => req.bearer_auth(t),
            None => req,
        }
    }

    /// GET an endpoint that returns JSON directly (capabilities, presets, health).
    pub fn get_json(&self, path: &str) -> Result<Value> {
        let resp = self
            .auth(self.http.get(self.url(path)))
            .send()
            .with_context(|| format!("GET {}", self.url(path)))?;
        read_json(resp)
    }

    /// Submit a multipart job (files + JSON `request` body), returning the job id.
    pub fn submit(&self, path: &str, files: &[&Path], request_body: &Value) -> Result<String> {
        let mut form = Form::new().text("request", serde_json::to_string(request_body)?);
        for (i, f) in files.iter().enumerate() {
            let field = if i == 0 {
                "file".to_string()
            } else {
                format!("file{}", i + 1)
            };
            form = form
                .file(field, f)
                .with_context(|| format!("attaching {}", f.display()))?;
        }

        let resp = self
            .auth(self.http.post(self.url(path)).multipart(form))
            .send()
            .with_context(|| format!("POST {}", self.url(path)))?;
        let json = read_json(resp)?;
        json.get("job_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("server did not return a job_id")
    }

    /// Poll a job to completion, driving a progress bar; returns the terminal job JSON.
    pub fn wait(&self, job_id: &str, quiet: bool) -> Result<Value> {
        let bar = if quiet {
            None
        } else {
            let b = ProgressBar::new(100);
            b.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% {msg}",
                )
                .unwrap()
                .progress_chars("#>-"),
            );
            Some(b)
        };

        loop {
            let status = self.get_json(&format!("jobs/{job_id}/status"))?;
            let state = status.get("status").and_then(Value::as_str).unwrap_or("");
            if let (Some(bar), Some(p)) = (&bar, status.get("progress").and_then(Value::as_f64)) {
                bar.set_position(p as u64);
            }
            match state {
                "completed" => {
                    if let Some(bar) = &bar {
                        bar.finish_with_message("done");
                    }
                    return Ok(status);
                }
                "failed" => {
                    if let Some(bar) = &bar {
                        bar.abandon_with_message("failed");
                    }
                    let err = status
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error");
                    bail!("job failed: {err}");
                }
                "cancelled" => bail!("job was cancelled"),
                _ => std::thread::sleep(Duration::from_millis(300)),
            }
        }
    }

    /// Download a completed job's binary result to `output`.
    pub fn download(&self, job_id: &str, output: &Path) -> Result<()> {
        let resp = self
            .auth(self.http.get(self.url(&format!("jobs/{job_id}/result"))))
            .send()
            .context("downloading result")?;
        if !resp.status().is_success() {
            bail!("download failed: HTTP {}", resp.status());
        }
        let bytes = resp.bytes().context("reading result body")?;
        std::fs::write(output, &bytes)
            .with_context(|| format!("writing {}", output.display()))?;
        Ok(())
    }

    /// Fetch a completed job's inline JSON result (e.g. probe).
    pub fn result_json(&self, job_id: &str) -> Result<Value> {
        self.get_json(&format!("jobs/{job_id}/result"))
    }

    /// Delete a job and its server-side files.
    pub fn delete(&self, job_id: &str) -> Result<()> {
        let _ = self
            .auth(self.http.delete(self.url(&format!("jobs/{job_id}"))))
            .send();
        Ok(())
    }
}

fn read_json(resp: reqwest::blocking::Response) -> Result<Value> {
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        bail!("HTTP {}: {}", status, text.trim());
    }
    serde_json::from_str(&text).with_context(|| format!("invalid JSON response: {text}"))
}
