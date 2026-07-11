#!/usr/bin/env bash
set -Eeuo pipefail

# ============================================================
# OSAI macOS development bootstrap
# ============================================================
# This prepares a Mac for local OSAI development/testing:
#   - Xcode Command Line Tools check
#   - Homebrew and build dependencies
#   - Docker Desktop and Docker Compose
#   - Rust through rustup
#   - OSAI repository checkout
#   - environment placeholders
#   - Qwen3-1.7B-Q8_0.gguf download and verification
#   - Cargo and Compose validation
#
# It intentionally does NOT install the Linux production service layer:
#   - no systemd/systemd-creds
#   - no Linux osai service account
#   - no firewalld configuration
#   - no Linux readiness setter execution
#
# macOS uses launchd rather than systemd. Add a LaunchAgent only after the
# Rust application itself has been verified to support macOS system scanning.
# ============================================================

SCRIPT_START_EPOCH="$(date +%s)"
SCRIPT_START_TIME="$(date '+%Y-%m-%d %H:%M:%S %Z')"
DURATION_REPORTED=0
CURRENT_STEP="initialization"
STEP_NAMES=()
STEP_SECONDS=()

LOG_DIR="${LOG_DIR:-$HOME/Library/Logs/osai}"
mkdir -p "$LOG_DIR"
LOG_FILE="${LOG_FILE:-$LOG_DIR/macos-bootstrap.log}"
exec > >(tee -a "$LOG_FILE") 2>&1

REPO_URL="${REPO_URL:-https://github.com/Maninder1220/OS.rs.git}"
BASE_DIR="${BASE_DIR:-$HOME/.local/share/osai}"
REPO_DIR="${REPO_DIR:-$BASE_DIR/OS.rs}"
APP_DIR="${APP_DIR:-$REPO_DIR/osai-agent}"
MODEL_DIR="${MODEL_DIR:-$APP_DIR/models}"

MODEL_FILE="${MODEL_FILE:-Qwen3-1.7B-Q8_0.gguf}"
MODEL_URL="${MODEL_URL:-https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q8_0.gguf?download=true}"
MODEL_SHA256="${MODEL_SHA256:-061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a}"
LEGACY_MODEL_FILE="${LEGACY_MODEL_FILE:-Qwen3-4B-Q4_K_M.gguf}"

DOWNLOAD_MODEL="${DOWNLOAD_MODEL:-1}"
RUN_CARGO_CHECK="${RUN_CARGO_CHECK:-1}"
RUN_CARGO_BUILD="${RUN_CARGO_BUILD:-0}"
STRICT_COMPOSE_CHECK="${STRICT_COMPOSE_CHECK:-0}"
INSTALL_DOCKER_DESKTOP="${INSTALL_DOCKER_DESKTOP:-1}"
DOCKER_WAIT_SECONDS="${DOCKER_WAIT_SECONDS:-240}"

ENV_STORAGE_SOURCE="${ENV_STORAGE_SOURCE:-$APP_DIR/.env.storage.example}"
ENV_COGNEE_SOURCE="${ENV_COGNEE_SOURCE:-$APP_DIR/.env.cognee.example}"

log() {
  printf '\n[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*"
}

warn() {
  printf '[WARN] %s\n' "$*" >&2
}

format_duration() {
  local total_seconds="${1:-0}"
  local hours minutes seconds
  hours=$((total_seconds / 3600))
  minutes=$(((total_seconds % 3600) / 60))
  seconds=$((total_seconds % 60))

  if [ "$hours" -gt 0 ]; then
    printf '%dh %02dm %02ds' "$hours" "$minutes" "$seconds"
  elif [ "$minutes" -gt 0 ]; then
    printf '%dm %02ds' "$minutes" "$seconds"
  else
    printf '%ds' "$seconds"
  fi
}

record_step() {
  STEP_NAMES[${#STEP_NAMES[@]}]="$1"
  STEP_SECONDS[${#STEP_SECONDS[@]}]="$2"
}

timed_step() {
  local name="$1"
  shift
  local started finished elapsed

  CURRENT_STEP="$name"
  started="$(date +%s)"
  log "STEP START: $name"
  "$@"
  finished="$(date +%s)"
  elapsed=$((finished - started))
  record_step "$name" "$elapsed"
  log "STEP COMPLETE: $name ($(format_duration "$elapsed"))"
}

report_duration() {
  local result="${1:-UNKNOWN}"
  [ "$DURATION_REPORTED" -eq 0 ] || return 0
  DURATION_REPORTED=1

  local finished total index
  finished="$(date +%s)"
  total=$((finished - SCRIPT_START_EPOCH))

  printf '\n============================================================\n'
  printf 'OSAI macOS bootstrap timing report\n'
  printf '============================================================\n'
  printf 'Result:       %s\n' "$result"
  printf 'Started:      %s\n' "$SCRIPT_START_TIME"
  printf 'Finished:     %s\n' "$(date '+%Y-%m-%d %H:%M:%S %Z')"
  printf 'Total time:   %s\n' "$(format_duration "$total")"
  printf 'Current step: %s\n' "$CURRENT_STEP"

  if [ "${#STEP_NAMES[@]}" -gt 0 ]; then
    printf '\nCompleted stage durations:\n'
    index=0
    while [ "$index" -lt "${#STEP_NAMES[@]}" ]; do
      printf '  - %-42s %s\n' \
        "${STEP_NAMES[$index]}" \
        "$(format_duration "${STEP_SECONDS[$index]}")"
      index=$((index + 1))
    done
  fi

  printf 'Log:          %s\n' "$LOG_FILE"
  printf '============================================================\n\n'
}

die() {
  printf '[ERROR] %s\n' "$*" >&2
  report_duration "FAILED"
  trap - ERR
  exit 1
}

on_error() {
  local code=$?
  local line="${1:-unknown}"
  printf '\n[ERROR] Script failed at line %s with exit code %s\n' "$line" "$code" >&2
  report_duration "FAILED"
  trap - ERR
  exit "$code"
}

trap 'on_error $LINENO' ERR

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

validate_macos() {
  [ "$(uname -s)" = "Darwin" ] || die "This script is only for macOS"
  [ "${EUID:-$(id -u)}" -ne 0 ] || die "Do not run this macOS script with sudo"

  local product_version major_version
  product_version="$(sw_vers -productVersion)"
  major_version="${product_version%%.*}"

  echo "macOS version: $product_version"
  echo "Architecture:  $(uname -m)"
  echo "Current user:  $(id -un)"

  if [ "$major_version" -lt 14 ]; then
    warn "Homebrew currently lists macOS Sonoma 14 or newer as its supported baseline"
  fi
}

ensure_xcode_clt() {
  if xcode-select -p >/dev/null 2>&1; then
    xcode-select -p
    return 0
  fi

  warn "Xcode Command Line Tools are not installed"
  xcode-select --install >/dev/null 2>&1 || true
  die "Complete the Apple Command Line Tools installer, then rerun this script"
}

configure_brew_shell() {
  if command_exists brew; then
    return 0
  fi

  if [ -x /opt/homebrew/bin/brew ]; then
    eval "$(/opt/homebrew/bin/brew shellenv)"
  elif [ -x /usr/local/bin/brew ]; then
    eval "$(/usr/local/bin/brew shellenv)"
  fi
}

install_homebrew() {
  configure_brew_shell
  if command_exists brew; then
    brew --version
    return 0
  fi

  log "Installing Homebrew using its official installer"
  NONINTERACTIVE=1 /bin/bash -c \
    "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

  configure_brew_shell
  command_exists brew || die "Homebrew installation completed but brew is not in PATH"
  brew --version
}

install_packages() {
  configure_brew_shell
  brew update
  brew install git jq pkg-config openssl@3 coreutils

  git --version
  jq --version
}

install_and_start_docker_desktop() {
  configure_brew_shell

  if ! command_exists docker && [ "$INSTALL_DOCKER_DESKTOP" = "1" ]; then
    log "Installing Docker Desktop through Homebrew cask"
    brew install --cask docker
  fi

  if [ -d /Applications/Docker.app ] || [ -d "$HOME/Applications/Docker.app" ]; then
    open -gja Docker || true
  fi

  local waited=0
  while ! docker info >/dev/null 2>&1; do
    if [ "$waited" -ge "$DOCKER_WAIT_SECONDS" ]; then
      die "Docker Desktop did not become ready within ${DOCKER_WAIT_SECONDS}s. Open Docker Desktop, finish setup, and rerun."
    fi
    printf '[INFO] Waiting for Docker Desktop... %ss/%ss\n' "$waited" "$DOCKER_WAIT_SECONDS"
    sleep 5
    waited=$((waited + 5))
  done

  docker --version
  docker compose version
}

install_rust() {
  if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
  fi

  if command_exists rustc && command_exists cargo && command_exists rustup; then
    rustc --version
    cargo --version
    rustup --version
    return 0
  fi

  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --profile minimal --default-toolchain stable

  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
  rustc --version
  cargo --version
  rustup --version
}

clone_or_update_repo() {
  mkdir -p "$BASE_DIR"

  if [ -d "$REPO_DIR/.git" ]; then
    cd "$REPO_DIR"
    git remote -v
    git status --short
    git pull --ff-only || warn "git pull failed; continuing with the existing checkout"
  else
    git clone "$REPO_URL" "$REPO_DIR"
  fi

  [ -d "$APP_DIR" ] || die "Expected app directory is missing: $APP_DIR. Ensure the repository/branch contains osai-agent or override APP_DIR."
}

prepare_env_files() {
  if [ ! -f "$APP_DIR/.env.storage" ]; then
    [ -f "$ENV_STORAGE_SOURCE" ] || die "Missing environment template: $ENV_STORAGE_SOURCE"
    cp "$ENV_STORAGE_SOURCE" "$APP_DIR/.env.storage"
  fi

  if [ ! -f "$APP_DIR/.env.cognee" ]; then
    [ -f "$ENV_COGNEE_SOURCE" ] || die "Missing environment template: $ENV_COGNEE_SOURCE"
    cp "$ENV_COGNEE_SOURCE" "$APP_DIR/.env.cognee"
  fi

  chmod 600 "$APP_DIR/.env.storage" "$APP_DIR/.env.cognee"
  warn "Review .env.storage and .env.cognee before starting the stack"
}

download_model() {
  if [ "$DOWNLOAD_MODEL" != "1" ]; then
    warn "Skipping model download because DOWNLOAD_MODEL=$DOWNLOAD_MODEL"
    return 0
  fi

  mkdir -p "$MODEL_DIR"
  cd "$MODEL_DIR"

  if [ ! -s "$MODEL_FILE" ]; then
    curl -fL --retry 5 --retry-delay 5 -C - \
      -o "$MODEL_FILE" \
      "$MODEL_URL"
  fi

  [ -s "$MODEL_FILE" ] || die "Model file is empty: $MODEL_DIR/$MODEL_FILE"

  local magic actual_sha256
  magic="$(dd if="$MODEL_FILE" bs=4 count=1 2>/dev/null || true)"
  [ "$magic" = "GGUF" ] || die "Model does not have a GGUF header: $MODEL_FILE"

  actual_sha256="$(shasum -a 256 "$MODEL_FILE" | awk '{print $1}')"
  [ "$actual_sha256" = "$MODEL_SHA256" ] || {
    printf '[ERROR] Expected SHA-256: %s\n' "$MODEL_SHA256" >&2
    printf '[ERROR] Actual SHA-256:   %s\n' "$actual_sha256" >&2
    die "Model checksum validation failed"
  }

  ln -sfn "$MODEL_FILE" "$MODEL_DIR/$LEGACY_MODEL_FILE"
  [ -s "$MODEL_DIR/$LEGACY_MODEL_FILE" ] || die "Legacy model compatibility path is invalid"

  ls -lh "$MODEL_FILE" "$LEGACY_MODEL_FILE"
}

verify_project() {
  # shellcheck disable=SC1090
  [ ! -f "$HOME/.cargo/env" ] || source "$HOME/.cargo/env"

  [ -f "$APP_DIR/Cargo.toml" ] || die "Cargo.toml missing: $APP_DIR/Cargo.toml"
  cd "$APP_DIR"
  cargo metadata --no-deps >/dev/null

  if [ "$RUN_CARGO_CHECK" = "1" ]; then
    cargo check
  fi

  if [ "$RUN_CARGO_BUILD" = "1" ]; then
    cargo build --release --bin osai-all
  fi

  if [ -f docker-compose.storage.yml ] && [ -f .env.storage ]; then
    if ! docker compose --env-file .env.storage -f docker-compose.storage.yml config >/dev/null; then
      if [ "$STRICT_COMPOSE_CHECK" = "1" ]; then
        die "Docker Compose storage configuration is invalid"
      fi
      warn "Compose validation failed; real .env values may still be required"
    fi
  fi
}

print_summary() {
  cat <<SUMMARY

============================================================
OSAI macOS development bootstrap complete
============================================================

Repository:
  $REPO_DIR

Application:
  $APP_DIR

Model:
  $MODEL_DIR/$MODEL_FILE

Compatibility model path:
  $MODEL_DIR/$LEGACY_MODEL_FILE

Docker:
  $(docker --version)
  $(docker compose version)

Important macOS limitation:
  This bootstrap prepares local development and Docker-based services.
  It does not install the Linux systemd credential/service layer, and it
  does not claim that Linux host-scanning code works natively on macOS.

Before running OSAI:
  1. Edit $APP_DIR/.env.storage
  2. Edit $APP_DIR/.env.cognee
  3. Verify the Rust scanner supports Darwin/macOS

Useful commands:
  cd "$APP_DIR"
  source "$HOME/.cargo/env"
  cargo check
  docker compose --env-file .env.storage -f docker-compose.storage.yml config

Log:
  $LOG_FILE

============================================================
SUMMARY
}

main() {
  log "OSAI macOS bootstrap started"
  log "Expected first-run duration: approximately 15-45 minutes"
  log "Expected repeat-run duration: approximately 2-10 minutes"

  timed_step "Validate macOS and current user" validate_macos
  timed_step "Check Apple Command Line Tools" ensure_xcode_clt
  timed_step "Install or verify Homebrew" install_homebrew
  timed_step "Install macOS build dependencies" install_packages
  timed_step "Install and start Docker Desktop" install_and_start_docker_desktop
  timed_step "Install or verify Rust toolchain" install_rust
  timed_step "Clone or update OSAI repository" clone_or_update_repo
  timed_step "Prepare OSAI environment files" prepare_env_files
  timed_step "Download and validate Qwen3 1.7B model" download_model
  timed_step "Validate Cargo and Docker Compose project" verify_project
  timed_step "Print macOS completion summary" print_summary

  CURRENT_STEP="bootstrap complete"
}

main "$@"
report_duration "SUCCESS"
