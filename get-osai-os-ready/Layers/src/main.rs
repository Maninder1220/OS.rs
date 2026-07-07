use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ============================================================
// OSAI Stage 2 Rust Infra Starter
// ============================================================
// What this binary does:
// 1. Finds the OSAI repo/app directory.
// 2. Copies .env.storage.example -> .env.storage when missing.
// 3. Copies .env.cognee.example -> .env.cognee when missing.
// 4. Asks the operator for important Cognee/local model values.
// 5. Updates .env files safely enough for simple KEY=value files.
// 6. Verifies Qwen GGUF model exists.
// 7. Starts the repo-owned docker-compose.storage.yml.
// 8. Builds Rust release binaries.
// 9. Optionally starts osai-all.
//
// Why Rust instead of only Bash:
// - Rust gives structured error handling with Result.
// - Paths are handled as PathBuf instead of fragile string concatenation.
// - Commands use std::process::Command with separated args, reducing shell bugs.
// - Env file updates are centralized and easier to test later.
// - This can become a single production binary.
// ============================================================

#[derive(Debug, Clone)]
struct Config {
    base_dir: PathBuf,
    repo_dir: PathBuf,
    app_dir: PathBuf,
    model_file: String,
    start_osai_all: bool,
}

#[derive(Debug, Clone)]
struct CogneeAnswers {
    llm_provider: String,
    llm_model: String,
    llm_endpoint: String,
    llm_api_key: String,
    osai_cognee_dataset: String,
    osai_gguf_model_file: String,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("[ERROR] {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = Config::from_env();

    log("OSAI Stage 2 Rust infra starter");
    println!("Base dir : {}", config.base_dir.display());
    println!("Repo dir : {}", config.repo_dir.display());
    println!("App dir  : {}", config.app_dir.display());

    require_repo(&config)?;
    prepare_env_files(&config)?;
    let answers = prompt_cognee_values(&config)?;
    update_env_files(&config, &answers)?;
    verify_model(&config)?;
    start_compose(&config)?;
    build_rust_binaries(&config)?;
    print_checks(&config);

    if config.start_osai_all {
        start_osai_all(&config)?;
    } else {
        warn("Skipping osai-all foreground start because OSAI_START_ALL=false.");
    }

    Ok(())
}

impl Config {
    fn from_env() -> Self {
        let base_dir = env_path("BASE_DIR", "/opt/osai");
        let repo_dir = env_path("REPO_DIR", &format!("{}/OS.rs", base_dir.display()));
        let app_dir = env_path("APP_DIR", &format!("{}/osai-agent", repo_dir.display()));
        let model_file = env::var("MODEL_FILE").unwrap_or_else(|_| "Qwen3-4B-Q4_K_M.gguf".to_string());
        let start_osai_all = env_bool("OSAI_START_ALL", true);

        Self {
            base_dir,
            repo_dir,
            app_dir,
            model_file,
            start_osai_all,
        }
    }
}

fn env_path(key: &str, default: &str) -> PathBuf {
    PathBuf::from(env::var(key).unwrap_or_else(|_| default.to_string()))
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(value) => matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "y"),
        Err(_) => default,
    }
}

fn log(message: &str) {
    println!("\n[INFO] {message}");
}

fn warn(message: &str) {
    eprintln!("[WARN] {message}");
}

fn require_repo(config: &Config) -> Result<(), String> {
    log("Checking repo structure");

    require_dir(&config.app_dir, "OSAI app directory")?;
    require_file(&config.app_dir.join("docker-compose.storage.yml"), "docker-compose.storage.yml")?;
    require_file(&config.app_dir.join("Cargo.toml"), "Cargo.toml")?;
    require_file(&config.app_dir.join(".env.storage.example"), ".env.storage.example")?;
    require_file(&config.app_dir.join(".env.cognee.example"), ".env.cognee.example")?;

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

fn prepare_env_files(config: &Config) -> Result<(), String> {
    log("Preparing .env files");

    copy_if_missing(
        &config.app_dir.join(".env.storage.example"),
        &config.app_dir.join(".env.storage"),
    )?;

    copy_if_missing(
        &config.app_dir.join(".env.cognee.example"),
        &config.app_dir.join(".env.cognee"),
    )?;

    Ok(())
}

fn copy_if_missing(src: &Path, dst: &Path) -> Result<(), String> {
    if dst.exists() {
        println!("KEEP {}", dst.display());
        return Ok(());
    }

    fs::copy(src, dst)
        .map_err(|err| format!("Failed to copy {} -> {}: {err}", src.display(), dst.display()))?;

    println!("COPY {} -> {}", src.display(), dst.display());
    Ok(())
}

fn prompt_cognee_values(config: &Config) -> Result<CogneeAnswers, String> {
    log("Asking for Cognee/local model values");
    println!("Press Enter to accept the default value.");
    println!("Defaults assume Cognee calls llama.cpp through Docker DNS: http://llama:8080/v1");

    Ok(CogneeAnswers {
        llm_provider: ask_default("LLM provider for Cognee", "custom")?,
        llm_model: ask_default("LLM model/alias", "osai-llm")?,
        llm_endpoint: ask_default("LLM endpoint from inside Cognee container", "http://llama:8080/v1")?,
        llm_api_key: ask_default("LLM API key for local llama.cpp", "not-needed-for-local-llama")?,
        osai_cognee_dataset: ask_default("Cognee dataset name", "osai_memory")?,
        osai_gguf_model_file: ask_default("GGUF model filename", &config.model_file)?,
    })
}

fn ask_default(prompt: &str, default_value: &str) -> Result<String, String> {
    print!("{prompt} [{default_value}]: ");
    io::stdout()
        .flush()
        .map_err(|err| format!("Failed to flush stdout: {err}"))?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|err| format!("Failed to read input: {err}"))?;

    let trimmed = answer.trim();
    if trimmed.is_empty() {
        Ok(default_value.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn update_env_files(config: &Config, answers: &CogneeAnswers) -> Result<(), String> {
    log("Updating .env.cognee and .env.storage");

    let cognee_env = config.app_dir.join(".env.cognee");
    upsert_env(&cognee_env, "LLM_PROVIDER", &answers.llm_provider)?;
    upsert_env(&cognee_env, "LLM_MODEL", &answers.llm_model)?;
    upsert_env(&cognee_env, "LLM_ENDPOINT", &answers.llm_endpoint)?;
    upsert_env(&cognee_env, "LLM_API_KEY", &answers.llm_api_key)?;
    upsert_env(&cognee_env, "OSAI_COGNEE_DATASET", &answers.osai_cognee_dataset)?;

    let storage_env = config.app_dir.join(".env.storage");
    upsert_env(&storage_env, "OSAI_GGUF_MODEL_FILE", &answers.osai_gguf_model_file)?;

    Ok(())
}

fn upsert_env(path: &Path, key: &str, value: &str) -> Result<(), String> {
    let existing = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;

    let mut found = false;
    let mut output = String::new();

    for line in existing.lines() {
        if line.starts_with(&format!("{key}=")) {
            output.push_str(&format!("{key}={value}\n"));
            found = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    if !found {
        output.push_str(&format!("{key}={value}\n"));
    }

    fs::write(path, output)
        .map_err(|err| format!("Failed to write {}: {err}", path.display()))?;

    println!("SET  {} in {}", key, path.display());
    Ok(())
}

fn verify_model(config: &Config) -> Result<(), String> {
    log("Checking Qwen GGUF model");

    let model_path = config.app_dir.join("models").join(&config.model_file);
    if !model_path.is_file() {
        return Err(format!(
            "Model missing: {}. Run Stage 1 first or download GGUF into models/.",
            model_path.display()
        ));
    }

    let metadata = fs::metadata(&model_path)
        .map_err(|err| format!("Failed to stat {}: {err}", model_path.display()))?;

    if metadata.len() == 0 {
        return Err(format!("Model exists but is empty: {}", model_path.display()));
    }

    println!("OK   {} ({} bytes)", model_path.display(), metadata.len());
    Ok(())
}

fn start_compose(config: &Config) -> Result<(), String> {
    log("Starting repo-owned Docker Compose stack");

    run_command(
        "docker",
        &["compose", "-f", "docker-compose.storage.yml", "up", "-d", "--build"],
        &config.app_dir,
    )?;

    run_command(
        "docker",
        &["compose", "-f", "docker-compose.storage.yml", "ps"],
        &config.app_dir,
    )?;

    Ok(())
}

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

fn start_osai_all(config: &Config) -> Result<(), String> {
    log("Starting osai-all supervisor in foreground");

    let binary = config.app_dir.join("target").join("release").join("osai-all");
    if !binary.exists() {
        warn("osai-all not found. Skipping foreground start.");
        return Ok(());
    }

    println!("Press Ctrl+C to stop osai-all.");

    let status = Command::new(binary)
        .current_dir(&config.app_dir)
        .env("RUST_LOG", env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| format!("Failed to start osai-all: {err}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("osai-all exited with status: {status}"))
    }
}

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
        Err(format!("{program} {} exited with status: {status}", args.join(" ")))
    }
}

fn print_checks(config: &Config) {
    log("Useful checks");
    println!(
        r#"
Docker services:
  cd "{app}"
  docker compose -f docker-compose.storage.yml ps

llama.cpp:
  curl http://127.0.0.1:8080/v1/models

Cognee:
  curl -I http://127.0.0.1:8001/docs

RustFS:
  curl -I http://127.0.0.1:9000

OSAI dashboard:
  curl http://127.0.0.1:8000/api/health
"#,
        app = config.app_dir.display()
    );
}
