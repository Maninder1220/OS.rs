use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ============================================================
// get-osai-os-ready / Layers / src / main.rs
// ============================================================
// Purpose:
//   Stage 2 is a Rust helper that checks the real OSAI app path,
//   verifies the downloaded GGUF model, and builds OSAI release binaries.
//
// This binary DOES:
//   1. Reads paths from environment variables or uses defaults.
//   2. Checks that /opt/osai/OS.rs/osai-agent exists.
//   3. Checks Cargo.toml exists in the real OSAI app.
//   4. Checks the Qwen GGUF model exists and is non-empty.
//   5. Runs cargo build --release inside the real OSAI app directory.
//   6. Prints manual next commands.
//
// This binary DOES NOT:
//   - Copy .env files.
//   - Ask for secrets.
//   - Edit .env files.
//   - Start Docker Compose.
//   - Start osai-all.
//
// Expected user:
//   Run this as the "osai" deploy user created by startersv.sh.
// ============================================================

// Runtime configuration for the Stage 2 helper.
// PathBuf is used so path handling stays filesystem-aware instead of raw string-only.
#[derive(Debug, Clone)]
struct Config {
    base_dir: PathBuf,
    repo_dir: PathBuf,
    app_dir: PathBuf,
    model_file: String,
}

fn main() {
    // Keep main small: run() does the real work and returns a readable error.
    if let Err(err) = run() {
        eprintln!("[ERROR] {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = Config::from_env();

    log("OSAI Stage 2 Rust build starter");

    println!("Base dir : {}", config.base_dir.display());
    println!("Repo dir : {}", config.repo_dir.display());
    println!("App dir  : {}", config.app_dir.display());
    println!("Model    : {}", config.model_file);

    require_repo(&config)?;
    verify_model(&config)?;
    build_rust_binaries(&config)?;
    print_checks(&config);

    log("Stage 2 build checks completed successfully");

    Ok(())
}

impl Config {
    fn from_env() -> Self {
        // Defaults match startersv.sh.
        // Override these only when your OSAI repo lives somewhere else.
        let base_dir = env_path("BASE_DIR", "/opt/osai");
        let repo_dir = env_path("REPO_DIR", &format!("{}/OS.rs", base_dir.display()));
        let app_dir = env_path("APP_DIR", &format!("{}/osai-agent", repo_dir.display()));

        let model_file = env::var("MODEL_FILE")
            .unwrap_or_else(|_| "Qwen3-4B-Q4_K_M.gguf".to_string());

        Self {
            base_dir,
            repo_dir,
            app_dir,
            model_file,
        }
    }
}

// Read a path from an environment variable, or fall back to a default.
fn env_path(key: &str, default: &str) -> PathBuf {
    PathBuf::from(env::var(key).unwrap_or_else(|_| default.to_string()))
}

fn log(message: &str) {
    println!("\n[INFO] {message}");
}

fn warn(message: &str) {
    eprintln!("[WARN] {message}");
}

// Check the real OSAI app directory.
// This only validates files needed for the build helper.
fn require_repo(config: &Config) -> Result<(), String> {
    log("Checking OSAI repo/app structure");

    require_dir(&config.base_dir, "base directory")?;
    require_dir(&config.repo_dir, "repo directory")?;
    require_dir(&config.app_dir, "OSAI app directory")?;

    require_file(&config.app_dir.join("Cargo.toml"), "Cargo.toml")?;

    // Compose file is useful to know about, but this helper does not run Compose.
    let compose_file = config.app_dir.join("docker-compose.storage.yml");
    if compose_file.is_file() {
        println!("OK   docker-compose.storage.yml: {}", compose_file.display());
    } else {
        warn("docker-compose.storage.yml not found. Skipping because this binary does not start Compose.");
    }

    Ok(())
}

fn require_dir(path: &Path, label: &str) -> Result<(), String> {
    if path.is_dir() {
        println!("OK   {label}: {}", path.display());
        Ok(())
    } else {
        Err(format!("Missing {label}: {}", path.display()))
    }
}

fn require_file(path: &Path, label: &str) -> Result<(), String> {
    if path.is_file() {
        println!("OK   {label}: {}", path.display());
        Ok(())
    } else {
        Err(format!("Missing {label}: {}", path.display()))
    }
}

// Verify that the model downloaded by Stage 1 exists.
// The GGUF magic-header check catches obvious wrong-file downloads.
fn verify_model(config: &Config) -> Result<(), String> {
    log("Checking Qwen GGUF model");

    let model_path = config.app_dir.join("models").join(&config.model_file);

    if !model_path.is_file() {
        return Err(format!(
            "Model missing: {}\nRun startersv.sh first or download GGUF into models/.",
            model_path.display()
        ));
    }

    let metadata = fs::metadata(&model_path)
        .map_err(|err| format!("Failed to stat {}: {err}", model_path.display()))?;

    if metadata.len() == 0 {
        return Err(format!("Model exists but is empty: {}", model_path.display()));
    }

    let size_gb = metadata.len() as f64 / 1024.0 / 1024.0 / 1024.0;

    println!("OK   {} ({:.2} GB)", model_path.display(), size_gb);

    let bytes = fs::read(&model_path)
        .map_err(|err| format!("Failed to read {}: {err}", model_path.display()))?;

    if bytes.len() >= 4 && &bytes[0..4] == b"GGUF" {
        println!("OK   GGUF magic header detected");
    } else {
        warn("GGUF magic header not detected. File may be incomplete or not a GGUF model.");
    }

    Ok(())
}

// Build release binaries for the real OSAI app.
// This runs inside APP_DIR, not inside this helper's project directory.
fn build_rust_binaries(config: &Config) -> Result<(), String> {
    log("Building Rust release binaries");

    run_command("cargo", &["build", "--release"], &config.app_dir)?;

    let release_dir = config.app_dir.join("target").join("release");
    println!("Release dir: {}", release_dir.display());

    for name in [
        "osai-agent",
        "osai-all",
        "osai-storage-worker",
        "osai-cognee-ingest",
        "osai-ask",
    ] {
        let path = release_dir.join(name);

        if path.exists() {
            println!("OK   {}", path.display());
        } else {
            println!("MISS {}", path.display());
        }
    }

    Ok(())
}

// Run a process without constructing one large shell string.
// This avoids quoting bugs and keeps args separated.
fn run_command(program: &str, args: &[&str], cwd: &Path) -> Result<(), String> {
    println!("+ {} {}", program, args.join(" "));

    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| format!("Failed to run {program}: {err}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} {} exited with status: {status}",
            args.join(" ")
        ))
    }
}

// Print only commands that match the current helper behavior.
// This helper does not start infra; it tells the operator what to run manually.
fn print_checks(config: &Config) {
    log("Useful next manual commands");

    println!(
        r#"
Go to the real OSAI app:
  cd "{app}"

Check env files manually:
  ls -la .env.storage .env.cognee

Edit env files manually when ready:
  nano .env.storage
  nano .env.cognee

Check Docker Compose config manually:
  docker compose --env-file .env.storage -f docker-compose.storage.yml config

Start storage manually when ready:
  docker compose --env-file .env.storage -f docker-compose.storage.yml up -d --build

Check storage containers manually:
  docker compose --env-file .env.storage -f docker-compose.storage.yml ps

Check osai-all supported flags:
  ./target/release/osai-all --help

Start OSAI manually when ready:
  export OSAI_AGENT_TOKEN="replace-with-a-long-random-token"
  RUST_LOG=info ./target/release/osai-all

Health check:
  curl http://127.0.0.1:8000/api/health

Important:
  Do not run sudo from inside the osai user shell.
  If root is needed, exit back to your normal admin user first.
"#,
        app = config.app_dir.display()
    );
}
