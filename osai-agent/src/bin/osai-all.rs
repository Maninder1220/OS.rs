// =============================================================================
// File: src/bin/osai-all.rs
// Purpose:
//   One-command supervisor for the full local OSAI runtime.
//
// Where this fits in OSAI:
//   Operators can run this single binary instead of manually starting osai-agent,
//   osai-storage-worker, and osai-cognee-ingest in separate terminals.
//
// Topics to know before editing:
//   Rust process supervision, Docker Compose service startup, signal handling, and OSAI runtime dependencies.
//
// Important operational notes:
//   This binary does not duplicate worker logic. It starts the existing release binaries after ensuring the RustFS bucket exists.
// =============================================================================

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "osai-all")]
#[command(about = "Start Docker support services, ensure RustFS bucket, and supervise all OSAI Rust workers")]
struct Args {
    /// Docker Compose file that starts PostgreSQL, RustFS, llama.cpp/Qwen, and Cognee.
    #[arg(long, default_value = "docker-compose.storage.yml")]
    compose_file: PathBuf,

    /// Dashboard/API bind address for osai-agent.
    #[arg(long, default_value = "0.0.0.0:8000")]
    bind: String,

    /// Skip Docker Compose startup and only supervise Rust binaries.
    #[arg(long)]
    skip_compose: bool,

    /// Skip RustFS bucket init. Use only when you already verified the bucket exists.
    #[arg(long)]
    skip_bucket_init: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "osai_all=info".to_string()))
        .compact()
        .init();

    load_env_files();
    let args = Args::parse();

    if !args.skip_compose {
        start_support_stack(&args.compose_file)?;
    }
    if !args.skip_bucket_init {
        ensure_rustfs_bucket(&args.compose_file)?;
    }

    let mut children = vec![
        start_osai_agent(&args.bind)?,
        start_worker("osai-storage-worker", &[])?,
        start_worker("osai-cognee-ingest", &[])?,
    ];

    info!("OSAI runtime is running. Press Ctrl-C to stop supervised Rust processes.");
    supervise(&mut children)
}

fn start_support_stack(compose_file: &Path) -> Result<()> {
    info!(compose = %compose_file.display(), "starting Docker support stack");
    run_status(
        "docker",
        &[
            "compose",
            "-f",
            compose_file.to_str().unwrap_or("docker-compose.storage.yml"),
            "up",
            "-d",
            "--build",
            "postgres",
            "rustfs",
            "llama",
            "cognee",
        ],
    )
}

fn ensure_rustfs_bucket(compose_file: &Path) -> Result<()> {
    info!("ensuring RustFS bucket exists through the project rustfs-init service");
    run_status(
        "docker",
        &[
            "compose",
            "-f",
            compose_file.to_str().unwrap_or("docker-compose.storage.yml"),
            "up",
            "rustfs-init",
        ],
    )
}

fn start_osai_agent(bind: &str) -> Result<Child> {
    let binary = sibling_binary("osai-agent")?;
    info!(binary = %binary.display(), bind, "starting osai-agent");
    Command::new(binary)
        .arg("--bind")
        .arg(bind)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to start osai-agent")
}

fn start_worker(name: &str, args: &[&str]) -> Result<Child> {
    let binary = sibling_binary(name)?;
    info!(binary = %binary.display(), "starting worker");
    Command::new(binary)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to start {name}"))
}

fn supervise(children: &mut [Child]) -> Result<()> {
    loop {
        let mut exited = None;
        for child in children.iter_mut() {
            if let Some(status) = child.try_wait().context("failed to poll child process")? {
                exited = Some((child.id(), status));
                break;
            }
        }
        if let Some((pid, status)) = exited {
            warn!(pid, %status, "supervised process exited; stopping remaining processes");
            stop_children(children);
            anyhow::bail!("supervised process exited: {status}");
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn stop_children(children: &mut [Child]) {
    for child in children {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn sibling_binary(name: &str) -> Result<PathBuf> {
    let current = std::env::current_exe().context("failed to locate current executable")?;
    let dir = current
        .parent()
        .context("current executable has no parent directory")?;
    let candidate = dir.join(name);
    if candidate.exists() {
        Ok(candidate)
    } else {
        anyhow::bail!(
            "missing sibling binary {}. Build all binaries first with: cargo build --release",
            candidate.display()
        )
    }
}

fn run_status(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to run {program}"))?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("{program} exited with {status}")
    }
}

fn load_env_files() {
    let _ = dotenvy::from_filename(".env.storage");
    let _ = dotenvy::from_filename(".env.cognee");
    let _ = dotenvy::dotenv();
}
