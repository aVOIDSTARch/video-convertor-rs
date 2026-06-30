//! media-convertor CLI — the control plane for the FFmpeg shell.
//!
//! Every operation is expressed as a [`UniversalRequest`]. By default it runs locally,
//! dispatched through the same handler the server's queue uses. With `--server <URL>` the
//! exact same request is submitted to the remote API instead. The CLI can also manage the
//! server lifecycle (`server start|stop|status`).

mod client;
mod local;
mod server_control;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use media_convertor_core::api_queue::{Method, UniversalRequest};
use media_convertor_core::operation::ConvertRequest;
use media_convertor_core::{Config, Container};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "media-convertor", version, about = "Comprehensive, secure FFmpeg shell — CLI control plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Talk to a remote server instead of running locally.
    #[arg(long, global = true, value_name = "URL")]
    server: Option<String>,

    /// Bearer token for the remote server.
    #[arg(long, global = true, value_name = "TOKEN")]
    token: Option<String>,

    /// Base work directory (queue, uploads, outputs, pidfile).
    #[arg(long, global = true, value_name = "DIR")]
    work_dir: Option<PathBuf>,

    /// Verbose logging.
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress progress output.
    #[arg(short, long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a media file (full transcode).
    Convert(ConvertArgs),
    /// Probe a media file and print stream/format info.
    Probe {
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// List the installed ffmpeg's capabilities (discovered at runtime).
    Capabilities {
        /// One of: all, encoders, decoders, filters, muxers, demuxers, pix_fmts, protocols.
        #[arg(default_value = "all")]
        kind: String,
        #[arg(long)]
        json: bool,
    },
    /// List built-in presets.
    Presets {
        #[arg(long)]
        json: bool,
    },
    /// Extract the audio track from a file.
    ExtractAudio {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Re-encode with this audio codec instead of copying.
        #[arg(long)]
        codec: Option<String>,
    },
    /// Extract a single thumbnail frame.
    Thumbnail {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Timestamp in seconds.
        #[arg(long, default_value = "0")]
        time: f64,
        /// Output width in pixels (height auto).
        #[arg(long)]
        width: Option<u32>,
    },
    /// Apply a single ffmpeg video filtergraph.
    Filter {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Filtergraph, e.g. "scale=640:-2,fps=30".
        #[arg(long)]
        graph: String,
    },
    /// Concatenate multiple inputs into one output.
    Concat {
        #[arg(short, long)]
        output: PathBuf,
        /// Input files, in order (2 or more).
        #[arg(required = true, num_args = 2..)]
        inputs: Vec<PathBuf>,
    },
    /// Raw ffmpeg passthrough (power users). The given args are run with the input and
    /// output paths substituted; only enabled with --enable-raw.
    Raw {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Allow raw passthrough (it is sandboxed but powerful).
        #[arg(long)]
        enable_raw: bool,
        /// ffmpeg arguments inserted between input and output.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Batch-convert from a manifest (lines: INPUT OUTPUT [PRESET]).
    Batch {
        manifest: PathBuf,
    },
    /// Manage the HTTP API server.
    Server {
        #[command(subcommand)]
        action: ServerAction,
    },
}

#[derive(Args)]
struct ConvertArgs {
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(long)]
    preset: Option<String>,
    #[arg(long)]
    format: Option<String>,
    #[arg(long)]
    video_codec: Option<String>,
    #[arg(long)]
    audio_codec: Option<String>,
    #[arg(long)]
    crf: Option<u8>,
    #[arg(long)]
    audio_bitrate: Option<String>,
    #[arg(long)]
    width: Option<u32>,
    #[arg(long)]
    height: Option<u32>,
    #[arg(long)]
    fps: Option<f64>,
    #[arg(long)]
    sample_rate: Option<u32>,
    #[arg(long)]
    channels: Option<u16>,
    #[arg(long)]
    no_video: bool,
    #[arg(long)]
    no_audio: bool,
    #[arg(long)]
    copy_video: bool,
    #[arg(long)]
    copy_audio: bool,
    #[arg(long)]
    encoder_preset: Option<String>,
    #[arg(long)]
    pixel_format: Option<String>,
    #[arg(long)]
    start: Option<f64>,
    #[arg(long)]
    end: Option<f64>,
    #[arg(long)]
    duration: Option<f64>,
}

#[derive(Subcommand)]
enum ServerAction {
    /// Start the server.
    Start {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        workers: Option<usize>,
        #[arg(long)]
        token: Option<String>,
        /// Enable the gated raw passthrough on the server.
        #[arg(long)]
        enable_raw: bool,
        /// Run in the foreground (default is background/daemon).
        #[arg(long)]
        foreground: bool,
    },
    /// Stop a background server.
    Stop,
    /// Report whether a server is running.
    Status,
}

/// Global context (everything from `Cli` except the subcommand), so helpers can borrow
/// it freely while the owned `Command` is dispatched.
struct Ctx {
    server: Option<String>,
    token: Option<String>,
    quiet: bool,
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let level = if cli.verbose { "debug" } else if cli.quiet { "warn" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut config = Config::default();
    if let Some(dir) = &cli.work_dir {
        config.work_dir = dir.clone();
    }

    let ctx = Ctx {
        server: cli.server,
        token: cli.token,
        quiet: cli.quiet,
        verbose: cli.verbose,
    };

    match cli.command {
        Command::Server { action } => run_server_action(action, config, &ctx),
        other => run_operation(other, &ctx, &config),
    }
}

/// Build a remote client if `--server` was given.
fn remote(ctx: &Ctx) -> Option<client::Client> {
    ctx.server
        .as_ref()
        .map(|url| client::Client::new(url, ctx.token.clone()))
        .transpose()
        .ok()
        .flatten()
}

fn run_operation(command: Command, ctx: &Ctx, config: &Config) -> Result<()> {
    match command {
        Command::Convert(args) => cmd_convert(args, ctx, config),
        Command::Probe { input, json } => cmd_probe(&input, json, ctx, config),
        Command::Capabilities { kind, json } => cmd_capabilities(&kind, json, ctx, config),
        Command::Presets { json } => cmd_presets(json, ctx, config),
        Command::ExtractAudio { input, output, codec } => {
            let mut body = json!({});
            if let Some(c) = codec {
                body["codec"] = json!(c);
            }
            if let Some(c) = Container::from_path(&output) {
                body["format"] = json!(c.extension());
            }
            run_file_op("extract-audio", &[&input], &output, body, ctx, config, "extracting audio")
        }
        Command::Thumbnail { input, output, time, width } => {
            let mut body = json!({ "time": time });
            if let Some(w) = width {
                body["width"] = json!(w);
            }
            run_file_op("thumbnail", &[&input], &output, body, ctx, config, "thumbnail")
        }
        Command::Filter { input, output, graph } => {
            let mut body = json!({ "graph": graph });
            if let Some(c) = Container::from_path(&output) {
                body["format"] = json!(c.extension());
            }
            run_file_op("filter", &[&input], &output, body, ctx, config, "filtering")
        }
        Command::Concat { output, inputs } => {
            let mut body = json!({});
            if let Some(c) = Container::from_path(&output) {
                body["format"] = json!(c.extension());
            }
            let refs: Vec<&Path> = inputs.iter().map(PathBuf::as_path).collect();
            run_file_op("concat", &refs, &output, body, ctx, config, "concatenating")
        }
        Command::Raw { input, output, enable_raw, args } => {
            cmd_raw(&input, &output, enable_raw, args, ctx, config)
        }
        Command::Batch { manifest } => cmd_batch(&manifest, ctx, config),
        Command::Server { .. } => unreachable!("handled in main"),
    }
}

fn cmd_convert(args: ConvertArgs, ctx: &Ctx, config: &Config) -> Result<()> {
    // Derive target container from -o extension when neither --format nor --preset is set.
    let format = args.format.clone().or_else(|| {
        if args.preset.is_none() {
            Container::from_path(&args.output).map(|c| c.extension().to_string())
        } else {
            None
        }
    });

    let cr = ConvertRequest {
        preset: args.preset,
        format,
        video_codec: args.video_codec,
        audio_codec: args.audio_codec,
        crf: args.crf,
        audio_bitrate: args.audio_bitrate,
        width: args.width,
        height: args.height,
        fps: args.fps,
        sample_rate: args.sample_rate,
        channels: args.channels,
        no_video: args.no_video,
        no_audio: args.no_audio,
        copy_video: args.copy_video,
        copy_audio: args.copy_audio,
        encoder_preset: args.encoder_preset,
        pixel_format: args.pixel_format,
        start: args.start,
        end: args.end,
        duration: args.duration,
    };
    let body = serde_json::to_value(&cr)?;
    run_file_op("convert", &[&args.input], &args.output, body, ctx, config, "converting")
}

fn cmd_raw(
    input: &Path,
    output: &Path,
    enable_raw: bool,
    user_args: Vec<String>,
    ctx: &Ctx,
    config: &Config,
) -> Result<()> {
    if !enable_raw && ctx.server.is_none() {
        anyhow::bail!("raw passthrough requires --enable-raw");
    }
    // Sandwich the user args between the input and output placeholders.
    let mut args: Vec<String> = vec!["-i".into(), "INPUT".into()];
    args.extend(user_args);
    args.push("OUTPUT".into());
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mkv")
        .to_string();
    let body = json!({ "args": args, "output_ext": ext });

    if let Some(remote) = remote(ctx) {
        let job = remote.submit("raw", &[input], &body)?;
        remote.wait(&job, ctx.quiet)?;
        remote.download(&job, output)?;
        remote.delete(&job)?;
        report_written(output);
        return Ok(());
    }

    let handler = local::build_handler(config, true)?;
    let req = UniversalRequest::post("raw")
        .with_body(body)
        .with_attachment(local::local_attachment("file", input)?);
    local::run(&handler, req, Some(output), ctx.quiet, "raw")?;
    report_written(output);
    Ok(())
}

/// Shared path for file-producing operations (local or remote).
fn run_file_op(
    op: &str,
    inputs: &[&Path],
    output: &Path,
    body: Value,
    ctx: &Ctx,
    config: &Config,
    label: &str,
) -> Result<()> {
    if let Some(remote) = remote(ctx) {
        let job = remote.submit(op, inputs, &body)?;
        remote.wait(&job, ctx.quiet)?;
        remote.download(&job, output)?;
        remote.delete(&job)?;
        report_written(output);
        return Ok(());
    }

    let handler = local::build_handler(config, true)?;
    let mut req = UniversalRequest::post(op).with_body(body);
    for (i, input) in inputs.iter().enumerate() {
        let field = if i == 0 { "file".to_string() } else { format!("file{}", i + 1) };
        req = req.with_attachment(local::local_attachment(&field, input)?);
    }
    local::run(&handler, req, Some(output), ctx.quiet, label)?;
    report_written(output);
    Ok(())
}

fn cmd_probe(input: &Path, _json: bool, ctx: &Ctx, config: &Config) -> Result<()> {
    let value = if let Some(remote) = remote(ctx) {
        let job = remote.submit("probe", &[input], &json!({}))?;
        remote.wait(&job, true)?;
        remote.result_json(&job)?
    } else {
        let handler = local::build_handler(config, true)?;
        let req = UniversalRequest::post("probe")
            .with_attachment(local::local_attachment("file", input)?);
        let result = local::run(&handler, req, None, true, "probe")?;
        result.body.unwrap_or(Value::Null)
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn cmd_capabilities(kind: &str, _json: bool, ctx: &Ctx, config: &Config) -> Result<()> {
    let value = if let Some(remote) = remote(ctx) {
        remote.get_json(&format!("capabilities?kind={kind}"))?
    } else {
        let handler = local::build_handler(config, true)?;
        let req = UniversalRequest::new(Method::Get, "capabilities").with_query("kind", kind);
        let result = local::run(&handler, req, None, true, "capabilities")?;
        result.body.unwrap_or(Value::Null)
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn cmd_presets(_json: bool, ctx: &Ctx, config: &Config) -> Result<()> {
    let value = if let Some(remote) = remote(ctx) {
        remote.get_json("presets")?
    } else {
        let handler = local::build_handler(config, true)?;
        let req = UniversalRequest::new(Method::Get, "presets");
        let result = local::run(&handler, req, None, true, "presets")?;
        result.body.unwrap_or(Value::Null)
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn cmd_batch(manifest: &Path, ctx: &Ctx, config: &Config) -> Result<()> {
    let content = std::fs::read_to_string(manifest)
        .with_context(|| format!("reading manifest {}", manifest.display()))?;
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    println!("batch: {} jobs", lines.len());
    for (i, line) in lines.iter().enumerate() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            eprintln!("  [{}] skipping invalid line: {line}", i + 1);
            continue;
        }
        let input = PathBuf::from(parts[0]);
        let output = PathBuf::from(parts[1]);
        let preset = parts.get(2).map(|s| s.to_string());
        println!("  [{}/{}] {} -> {}", i + 1, lines.len(), parts[0], parts[1]);

        let format = if preset.is_none() {
            Container::from_path(&output).map(|c| c.extension().to_string())
        } else {
            None
        };
        let cr = ConvertRequest {
            preset,
            format,
            ..Default::default()
        };
        let body = serde_json::to_value(&cr)?;
        run_file_op("convert", &[&input], &output, body, ctx, config, "converting")?;
    }
    println!("batch complete.");
    Ok(())
}

fn run_server_action(action: ServerAction, mut config: Config, ctx: &Ctx) -> Result<()> {
    match action {
        ServerAction::Start {
            host,
            port,
            workers,
            token,
            enable_raw,
            foreground,
        } => {
            if let Some(h) = host {
                config.host = h;
            }
            if let Some(p) = port {
                config.port = p;
            }
            if let Some(w) = workers {
                config.workers = w;
            }
            config.token = token.or(ctx.token.clone());
            config.raw_enabled = enable_raw;
            server_control::start(
                &config,
                &server_control::StartOptions {
                    foreground,
                    quiet: ctx.quiet,
                    verbose: ctx.verbose,
                },
            )
        }
        ServerAction::Stop => server_control::stop(&config),
        ServerAction::Status => server_control::status(&config),
    }
}

fn report_written(output: &Path) {
    eprintln!("wrote {}", output.display());
}
