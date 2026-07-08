# get-osai-os-ready

`get-osai-os-ready` prepares a fresh Linux server for the OSAI app and then verifies/builds the OSAI Rust binaries.

This kit has two layers:

1. `startersv.sh` prepares the operating system.
2. `src/main.rs` verifies the OSAI app path, checks the GGUF model, and builds release binaries.

It does **not** start the full OSAI stack automatically. Docker Compose startup and `osai-all` startup stay manual so you can review `.env` values first.

## Folder structure

```text
get-osai-os-ready/
├── Layers/
│   ├── Cargo.toml
│   ├── src/
│   │   └── main.rs
│   └── startersv.sh
└── README.md
```

## File responsibilities

### `Layers/startersv.sh`

Stage 1 OS preparation script.

It does:

- detects Ubuntu/Debian or RHEL-family Linux
- installs base tools like `git`, `curl`, `jq`, compilers, OpenSSL headers, and network/process tools
- installs Docker Engine and the Docker Compose plugin
- creates a dedicated `osai` deploy user
- installs Rust under `/home/osai`, not under `/root`
- clones or updates the OSAI repo into `/opt/osai/OS.rs`
- creates `.env.storage` and `.env.cognee` from repo examples if they are missing
- downloads the Qwen GGUF model into `/opt/osai/OS.rs/osai-agent/models`
- fixes ownership so `/opt/osai` belongs to the `osai` user
- runs readiness checks

It does not:

- open firewall ports unless `FIREWALL_OPEN=1` is set

### `Layers/src/main.rs`

Stage 2 Rust build helper.

It does:

- reads `BASE_DIR`, `REPO_DIR`, `APP_DIR`, and `MODEL_FILE` from environment variables or uses defaults
- verifies `/opt/osai/OS.rs/osai-agent` exists
- verifies `Cargo.toml` exists in the OSAI app
- verifies the Qwen GGUF model exists and is not empty
- checks the `GGUF` file header
- runs `cargo build --release` inside the real OSAI app directory
- prints manual next commands


## Why the `osai` user is important

The script creates a dedicated deploy user:

```bash
osai
```

Use this user for app work:

```bash
sudo -iu osai
```

The `osai` user owns:

```text
/opt/osai
/home/osai/.cargo
/home/osai/.rustup
```

This keeps Rust builds, model files, and app files away from root.

Use root or your cloud admin user only for OS-level work like installing packages, changing firewall rules, or managing system services.

Do not run this from inside the `osai` shell:

```bash
sudo some-command
```

The `osai` user is not meant to be a sudo/admin user and normally has no password. If you need root, exit back to your normal admin user first:

```bash
exit
```

Then run the sudo command from your admin user.

## Stage 1: prepare the OS

From the project folder:

```bash
cd get-osai-os-ready/Layers
chmod +x startersv.sh
sudo bash startersv.sh
```

Useful options:

```bash
# Skip the large model download
DOWNLOAD_MODEL=0 sudo bash startersv.sh

# Skip cargo check during OS preparation
RUN_CARGO_CHECK=0 sudo bash startersv.sh

# Open common local app ports in firewalld
FIREWALL_OPEN=1 sudo bash startersv.sh

# Treat Docker Compose config failure as fatal
STRICT_COMPOSE_CHECK=1 sudo bash startersv.sh
```

Default installed/cloned paths:

```text
BASE_DIR=/opt/osai
REPO_DIR=/opt/osai/OS.rs
APP_DIR=/opt/osai/OS.rs/osai-agent
MODEL_DIR=/opt/osai/OS.rs/osai-agent/models
MODEL_FILE=Qwen3-4B-Q4_K_M.gguf
```

## Stage 2: run the Rust build helper

Make the kit available to the `osai` user. One clean way is to copy it under `/opt/osai`:

```bash
sudo cp -r get-osai-os-ready /opt/osai/get-osai-os-ready
sudo chown -R osai:osai /opt/osai/get-osai-os-ready
```

Switch to the deploy user:

```bash
sudo -iu osai
```

Run the Rust helper:

```bash
cd /opt/osai/get-osai-os-ready/Layers
source ~/.cargo/env
cargo run --release
```

The helper builds the real OSAI app at:

```text
/opt/osai/OS.rs/osai-agent
```

## Manual commands after Stage 2

Go to the real OSAI app:

```bash
cd /opt/osai/OS.rs/osai-agent
```

Review env files:

```bash
ls -la .env.storage .env.cognee
nano .env.storage
nano .env.cognee
```


Check supported `osai-all` options:

```bash
./target/release/osai-all --help
```

Start OSAI manually with a token:

```bash
export OSAI_AGENT_TOKEN="replace-with-a-long-random-token"
RUST_LOG=info ./target/release/osai-all
```

Do not use an unsupported flag. If `--help` does not show a flag, do not pass it.

## Environment overrides

Both files use the same default path values.

```bash
BASE_DIR=/custom/base
REPO_DIR=/custom/base/OS.rs
APP_DIR=/custom/base/OS.rs/osai-agent
MODEL_FILE=Qwen3-4B-Q4_K_M.gguf
```

Example:

```bash
APP_DIR=/opt/osai/OS.rs/osai-agent cargo run --release
```

