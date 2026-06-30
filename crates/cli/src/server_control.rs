//! Server lifecycle control: start (foreground/background), stop, and status.
//!
//! These commands are CLI-only — they are deliberately *not* exposed over the HTTP API,
//! which may only run ffmpeg operations. The CLI launches the `media-convertor-server`
//! binary, passing configuration via environment variables, and tracks it with a pidfile.

use anyhow::{bail, Context, Result};
use media_convertor_core::Config;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Options controlling how the server is launched.
pub struct StartOptions {
    pub foreground: bool,
    pub quiet: bool,
    pub verbose: bool,
}

/// Locate the sibling `media-convertor-server` binary (same dir as this CLI).
fn server_binary() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating current executable")?;
    let dir = exe.parent().context("executable has no parent directory")?;
    let candidate = dir.join("media-convertor-server");
    if candidate.exists() {
        Ok(candidate)
    } else {
        // Fall back to PATH resolution.
        Ok(PathBuf::from("media-convertor-server"))
    }
}

/// Build the environment the server process reads its config from.
fn server_env(config: &Config) -> Vec<(String, String)> {
    let mut env = vec![
        ("MEDIA_CONVERTOR_HOST".into(), config.host.clone()),
        ("MEDIA_CONVERTOR_PORT".into(), config.port.to_string()),
        ("MEDIA_CONVERTOR_WORKERS".into(), config.workers.to_string()),
        ("MEDIA_CONVERTOR_DATA".into(), config.work_dir.display().to_string()),
        ("MEDIA_CONVERTOR_TIMEOUT".into(), config.job_timeout_secs.to_string()),
        ("MEDIA_CONVERTOR_RAW".into(), if config.raw_enabled { "1".into() } else { "0".into() }),
    ];
    if let Some(token) = &config.token {
        env.push(("MEDIA_CONVERTOR_TOKEN".into(), token.clone()));
    }
    env
}

/// Read a running server's PID from the pidfile, if the process is alive.
fn running_pid(config: &Config) -> Option<u32> {
    let pid_str = std::fs::read_to_string(config.pid_file()).ok()?;
    let pid: u32 = pid_str.trim().parse().ok()?;
    if process_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

/// Whether a process with `pid` is alive (signal 0 probe).
fn process_alive(pid: u32) -> bool {
    // kill(pid, 0) returns 0 if the process exists and we can signal it.
    unsafe { kill(pid as i32, 0) == 0 }
}

// Minimal libc bindings to avoid pulling in a dependency for two syscalls.
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn setsid() -> i32;
}

/// Start the server, either in the foreground (blocking) or detached in the background.
pub fn start(config: &Config, opts: &StartOptions) -> Result<()> {
    config.validate_server()?;
    config.ensure_dirs().context("creating work directories")?;

    if let Some(pid) = running_pid(config) {
        bail!("server already running (pid {pid}); use `server stop` first");
    }

    let bin = server_binary()?;
    let env = server_env(config);
    let log_level = if opts.verbose {
        "debug"
    } else if opts.quiet {
        "warn"
    } else {
        "info"
    };

    if opts.foreground {
        let mut cmd = Command::new(&bin);
        cmd.envs(env).env("RUST_LOG", log_level);
        let status = cmd
            .status()
            .with_context(|| format!("running {}", bin.display()))?;
        if !status.success() {
            bail!("server exited with {status}");
        }
        return Ok(());
    }

    // Background: detach into a new session, redirect output to the logfile.
    let log = File::create(config.log_file()).context("opening server logfile")?;
    let log_err = log.try_clone().context("cloning logfile handle")?;

    let mut cmd = Command::new(&bin);
    cmd.envs(env)
        .env("RUST_LOG", log_level)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    // Detach from the controlling terminal so it survives the CLI exiting.
    unsafe {
        cmd.pre_exec(|| {
            // Create a new session; ignore failure (already a session leader).
            let _ = setsid();
            Ok(())
        });
    }

    let child = cmd
        .spawn()
        .with_context(|| format!("spawning {}", bin.display()))?;
    let pid = child.id();
    std::fs::write(config.pid_file(), pid.to_string()).context("writing pidfile")?;

    println!(
        "media-convertor server started in background (pid {pid}) on {}:{}",
        config.host, config.port
    );
    println!("logs: {}", config.log_file().display());
    Ok(())
}

/// Stop a background server identified by the pidfile.
pub fn stop(config: &Config) -> Result<()> {
    let pid_file = config.pid_file();
    let pid: u32 = match std::fs::read_to_string(&pid_file) {
        Ok(s) => s.trim().parse().context("invalid pidfile contents")?,
        Err(_) => bail!("no pidfile found at {}; is the server running?", pid_file.display()),
    };

    if !process_alive(pid) {
        let _ = std::fs::remove_file(&pid_file);
        bail!("server (pid {pid}) is not running; removed stale pidfile");
    }

    // SIGTERM for graceful shutdown.
    unsafe {
        kill(pid as i32, 15);
    }
    let _ = std::fs::remove_file(&pid_file);
    println!("sent shutdown signal to server (pid {pid})");
    Ok(())
}

/// Report whether a server is running.
pub fn status(config: &Config) -> Result<()> {
    match running_pid(config) {
        Some(pid) => {
            println!(
                "running (pid {pid}) — configured bind {}:{}, work dir {}",
                config.host,
                config.port,
                config.work_dir.display()
            );
        }
        None => println!("not running"),
    }
    Ok(())
}
