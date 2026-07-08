#!/usr/bin/env bash
set -Eeuo pipefail

# ============================================================
# get-osai-os-ready / Layers / startersv.sh
# ============================================================
# Purpose:
#   Stage 1 prepares a fresh Linux OS for the real OSAI app.
#
# This script DOES:
#   1. Detect Ubuntu/Debian or RHEL-family Linux.
#   2. Install base build/debug tools.
#   3. Install Docker Engine and Docker Compose plugin.
#   4. Create a dedicated deploy user named "osai" by default.
#   5. Install Rust for the deploy user, not for root.
#   6. Clone or update the OSAI repo into /opt/osai/OS.rs.
#   7. Create .env placeholder files from repo examples if missing.
#   8. Download the Qwen GGUF model into the OSAI app models directory.
#   9. Fix ownership and run readiness checks.
#
#
# Why create the "osai" user?
#   Root installs OS packages. The osai user owns and runs the app.
#   This keeps app files, Rust build artifacts, and model files away from root.
# ============================================================

# ----------------------------
# Operator-tunable settings
# ----------------------------
REPO_URL="${REPO_URL:-https://github.com/Maninder1220/OS.rs.git}"
OSAI_USER="${OSAI_USER:-osai}"

BASE_DIR="${BASE_DIR:-/opt/osai}"
REPO_DIR="${REPO_DIR:-$BASE_DIR/OS.rs}"
APP_DIR="${APP_DIR:-$REPO_DIR/osai-agent}"

MODEL_FILE="${MODEL_FILE:-Qwen3-4B-Q4_K_M.gguf}"
MODEL_URL="${MODEL_URL:-https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf?download=true}"
MODEL_DIR="${MODEL_DIR:-$APP_DIR/models}"

# FIREWALL_OPEN=1 opens common local app ports in firewalld.
# Default 0 is safer for cloud VMs.
FIREWALL_OPEN="${FIREWALL_OPEN:-0}"

# DOWNLOAD_MODEL=0 skips the large GGUF model download.
DOWNLOAD_MODEL="${DOWNLOAD_MODEL:-1}"

# STRICT_COMPOSE_CHECK=1 makes Docker Compose config failure fatal.
# Default 0 treats it as a warning because real .env values may not be ready yet.
STRICT_COMPOSE_CHECK="${STRICT_COMPOSE_CHECK:-0}"

# RUN_CARGO_CHECK=0 skips cargo check during OS preparation.
RUN_CARGO_CHECK="${RUN_CARGO_CHECK:-1}"

# ----------------------------
# Logging and error helpers
# ----------------------------
log() {
  printf '\n[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*"
}

warn() {
  printf '[WARN] %s\n' "$*" >&2
}

die() {
  printf '[ERROR] %s\n' "$*" >&2
  exit 1
}

on_error() {
  local exit_code=$?
  local line_no="${1:-unknown}"
  printf '\n[ERROR] Script failed at line %s with exit code %s\n' "$line_no" "$exit_code" >&2
  exit "$exit_code"
}

trap 'on_error $LINENO' ERR

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

require_root() {
  if [ "${EUID}" -ne 0 ]; then
    die "Run as root: sudo bash startersv.sh"
  fi
}

# ----------------------------
# Detect OS from /etc/os-release
# ----------------------------
detect_os() {
  [ -f /etc/os-release ] || die "/etc/os-release not found. This script is for Linux."

  # shellcheck disable=SC1091
  . /etc/os-release

  OS_ID="${ID:-unknown}"
  OS_LIKE="${ID_LIKE:-}"
  OS_NAME="${PRETTY_NAME:-$OS_ID}"
  OS_VERSION_ID="${VERSION_ID:-unknown}"
  OS_CODENAME="${VERSION_CODENAME:-}"

  log "Detected OS: $OS_NAME"
  echo "OS_ID=$OS_ID"
  echo "OS_LIKE=$OS_LIKE"
  echo "OS_VERSION_ID=$OS_VERSION_ID"
  echo "Architecture=$(uname -m)"
}

# ----------------------------
# Install common packages needed by Docker, Rust builds, Git, and debugging.
# ----------------------------
install_base_packages() {
  log "Installing base OS packages"

  if command_exists apt-get; then
    export DEBIAN_FRONTEND=noninteractive

    apt-get update
    apt-get install -y \
      ca-certificates curl gnupg lsb-release git unzip tar jq \
      build-essential pkg-config openssl libssl-dev \
      iproute2 procps lsof net-tools \
      sudo

  elif command_exists dnf; then
    dnf install -y \
      ca-certificates curl git unzip tar jq \
      gcc gcc-c++ make pkgconf-pkg-config openssl openssl-devel \
      iproute procps-ng lsof net-tools \
      dnf-plugins-core shadow-utils sudo

  elif command_exists yum; then
    yum install -y \
      ca-certificates curl git unzip tar jq \
      gcc gcc-c++ make pkgconfig openssl openssl-devel \
      iproute procps-ng lsof net-tools \
      yum-utils shadow-utils sudo

  else
    die "No supported package manager found. Need apt-get, dnf, or yum."
  fi
}

# ----------------------------
# Helper for adding Docker repo on dnf-based systems.
# Some dnf versions use --add-repo; newer plugin versions can use addrepo.
# ----------------------------
add_dnf_repo() {
  local repo_url="$1"

  if dnf config-manager --add-repo "$repo_url" >/dev/null 2>&1; then
    return 0
  fi

  if dnf config-manager addrepo --from-repofile="$repo_url" >/dev/null 2>&1; then
    return 0
  fi

  die "Failed to add DNF repo: $repo_url"
}

# ----------------------------
# Install Docker Engine and Docker Compose plugin.
# ----------------------------
install_docker() {
  log "Installing Docker Engine + Docker Compose plugin"

  if command_exists docker && docker compose version >/dev/null 2>&1; then
    log "Docker and Docker Compose plugin already installed"
    docker --version
    docker compose version
    systemctl enable --now docker || true
    return 0
  fi

  if command_exists apt-get; then
    install -m 0755 -d /etc/apt/keyrings

    curl -fsSL "https://download.docker.com/linux/${OS_ID}/gpg" \
      -o /etc/apt/keyrings/docker.asc

    chmod a+r /etc/apt/keyrings/docker.asc

    local codename
    codename="${OS_CODENAME:-$(lsb_release -cs)}"

    cat >/etc/apt/sources.list.d/docker.list <<EOF_DOCKER_APT
deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/${OS_ID} ${codename} stable
EOF_DOCKER_APT

    apt-get update
    apt-get install -y \
      docker-ce docker-ce-cli containerd.io \
      docker-buildx-plugin docker-compose-plugin

  elif command_exists dnf; then
    dnf -y install dnf-plugins-core || true

    # Remove old/conflicting Docker package names if present.
    # This intentionally avoids removing normal podman.
    dnf remove -y \
      docker docker-client docker-client-latest docker-common docker-latest \
      docker-latest-logrotate docker-logrotate docker-engine podman-docker \
      runc || true

    case "$OS_ID" in
      rhel)
        add_dnf_repo "https://download.docker.com/linux/rhel/docker-ce.repo"
        ;;
      centos|rocky|almalinux)
        add_dnf_repo "https://download.docker.com/linux/centos/docker-ce.repo"
        ;;
      *)
        if echo "$OS_LIKE" | grep -Eq 'rhel|centos|fedora'; then
          add_dnf_repo "https://download.docker.com/linux/centos/docker-ce.repo"
        else
          die "Unsupported dnf OS for Docker repo: $OS_ID / $OS_LIKE"
        fi
        ;;
    esac

    dnf install -y \
      docker-ce docker-ce-cli containerd.io \
      docker-buildx-plugin docker-compose-plugin

  elif command_exists yum; then
    yum -y install yum-utils || true

    yum remove -y \
      docker docker-client docker-client-latest docker-common docker-latest \
      docker-latest-logrotate docker-logrotate docker-engine podman-docker \
      runc || true

    yum-config-manager --add-repo https://download.docker.com/linux/centos/docker-ce.repo

    yum install -y \
      docker-ce docker-ce-cli containerd.io \
      docker-buildx-plugin docker-compose-plugin
  fi

  systemctl enable --now docker

  docker --version
  docker compose version
}

# ----------------------------
# Create the dedicated deploy user.
# Important:
#   - The osai user is not a sudo/admin user.
#   - Use sudo -iu osai from your normal admin user.
#   - Do not run sudo commands from inside the osai shell.
# ----------------------------
create_osai_user() {
  log "Creating/checking dedicated deploy user: $OSAI_USER"

  if id "$OSAI_USER" >/dev/null 2>&1; then
    log "User already exists: $OSAI_USER"
  else
    useradd --create-home --shell /bin/bash "$OSAI_USER"
    log "Created user: $OSAI_USER"
  fi

  mkdir -p "$BASE_DIR"
  chown -R "$OSAI_USER:$OSAI_USER" "$BASE_DIR"

  if getent group docker >/dev/null 2>&1; then
    usermod -aG docker "$OSAI_USER"
    log "Added $OSAI_USER to docker group"
  else
    warn "docker group not found. Docker may require sudo for this user."
  fi
}

# ----------------------------
# Install Rust with rustup for the osai user.
# This avoids /root/.cargo path problems and keeps build cache owned by osai.
# ----------------------------
install_rust_for_osai_user() {
  log "Installing/checking Rust for user: $OSAI_USER"

  local osai_home
  osai_home="$(getent passwd "$OSAI_USER" | cut -d: -f6)"

  if [ -z "$osai_home" ] || [ ! -d "$osai_home" ]; then
    die "Home directory for user $OSAI_USER not found"
  fi

  chown -R "$OSAI_USER:$OSAI_USER" "$osai_home"

  if sudo -iu "$OSAI_USER" bash <<'OSAI_RUST_CHECK_EOF'
set -Eeuo pipefail

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
fi

command -v rustc >/dev/null 2>&1
command -v cargo >/dev/null 2>&1
command -v rustup >/dev/null 2>&1

rustc --version
cargo --version
rustup --version
OSAI_RUST_CHECK_EOF
  then
    log "Rust already installed and working for user: $OSAI_USER"
    return 0
  fi

  log "Rust not found or incomplete for $OSAI_USER. Installing with rustup minimal profile."

  sudo -iu "$OSAI_USER" bash <<'OSAI_RUST_INSTALL_EOF'
set -Eeuo pipefail

export CARGO_HOME="$HOME/.cargo"
export RUSTUP_HOME="$HOME/.rustup"
export PATH="$CARGO_HOME/bin:$PATH"

mkdir -p "$CARGO_HOME" "$RUSTUP_HOME"

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y \
      --profile minimal \
      --default-toolchain stable \
      --no-modify-path

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
else
  export PATH="$HOME/.cargo/bin:$PATH"
fi

touch "$HOME/.profile" "$HOME/.bashrc"

if ! grep -q 'HOME/.cargo/bin' "$HOME/.profile"; then
  cat >>"$HOME/.profile" <<'PROFILE_EOF'

# Rust toolchain
if [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
PROFILE_EOF
fi

if ! grep -q 'HOME/.cargo/bin' "$HOME/.bashrc"; then
  cat >>"$HOME/.bashrc" <<'BASHRC_EOF'

# Rust toolchain
if [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
BASHRC_EOF
fi

rustup default stable
rustup show
rustup toolchain list
rustc --version
cargo --version
rustup --version
OSAI_RUST_INSTALL_EOF

  log "Rust installation completed for user: $OSAI_USER"
}

# ----------------------------
# Clone or update the real OSAI repo.
# ----------------------------
clone_repo() {
  log "Cloning or updating OSAI repo"

  mkdir -p "$BASE_DIR"
  chown -R "$OSAI_USER:$OSAI_USER" "$BASE_DIR"

  if [ -d "$REPO_DIR/.git" ]; then
    log "Repo already exists: $REPO_DIR"

    sudo -iu "$OSAI_USER" env REPO_DIR="$REPO_DIR" bash <<'OSAI_GIT_PULL_EOF'
set -Eeuo pipefail
cd "$REPO_DIR"
git remote -v
git status --short
git pull --ff-only || {
  echo "[WARN] git pull failed. Local changes may exist. Continuing with current checkout." >&2
}
OSAI_GIT_PULL_EOF
  else
    sudo -iu "$OSAI_USER" env REPO_URL="$REPO_URL" REPO_DIR="$REPO_DIR" bash <<'OSAI_GIT_CLONE_EOF'
set -Eeuo pipefail
git clone "$REPO_URL" "$REPO_DIR"
OSAI_GIT_CLONE_EOF
  fi

  [ -d "$APP_DIR" ] || die "Expected app directory missing: $APP_DIR"

  chown -R "$OSAI_USER:$OSAI_USER" "$BASE_DIR"
}

# ----------------------------
# Create placeholder env files from repo examples if missing.
# This is convenience only. Real secret values still need manual review.
# ----------------------------
prepare_env_placeholders() {
  log "Preparing env placeholder files if missing"

  [ -d "$APP_DIR" ] || die "APP_DIR missing: $APP_DIR"

  cd "$APP_DIR"

  if [ -f ".env.storage.example" ] && [ ! -f ".env.storage" ]; then
    cp .env.storage.example .env.storage
    chown "$OSAI_USER:$OSAI_USER" .env.storage
    chmod 600 .env.storage
    log "Created .env.storage from .env.storage.example"
  elif [ -f ".env.storage" ]; then
    log ".env.storage already exists; keeping existing file"
  else
    warn ".env.storage.example missing; could not create .env.storage"
  fi

  if [ -f ".env.cognee.example" ] && [ ! -f ".env.cognee" ]; then
    cp .env.cognee.example .env.cognee
    chown "$OSAI_USER:$OSAI_USER" .env.cognee
    chmod 600 .env.cognee
    warn "Created .env.cognee from example. Replace with real values before starting infra."
  elif [ -f ".env.cognee" ]; then
    log ".env.cognee already exists; keeping existing file"
  else
    warn ".env.cognee.example missing; could not create .env.cognee"
  fi
}

# ----------------------------
# Download the GGUF model into the real OSAI app's models directory.
# curl -C - allows resume if the download is interrupted.
# ----------------------------
download_qwen_model() {
  if [ "$DOWNLOAD_MODEL" != "1" ]; then
    log "Skipping model download because DOWNLOAD_MODEL=$DOWNLOAD_MODEL"
    return 0
  fi

  log "Preparing Qwen GGUF model"

  mkdir -p "$MODEL_DIR"
  chown -R "$OSAI_USER:$OSAI_USER" "$MODEL_DIR"

  sudo -iu "$OSAI_USER" env \
    MODEL_DIR="$MODEL_DIR" \
    MODEL_FILE="$MODEL_FILE" \
    MODEL_URL="$MODEL_URL" \
    bash <<'OSAI_MODEL_EOF'
set -Eeuo pipefail

mkdir -p "$MODEL_DIR"
cd "$MODEL_DIR"

if [ -s "$MODEL_FILE" ]; then
  echo "[INFO] Model already exists: $MODEL_DIR/$MODEL_FILE"
  ls -lh "$MODEL_FILE"
  exit 0
fi

echo "[INFO] Downloading model. If interrupted, rerun this script to resume."
echo "[INFO] Target: $MODEL_DIR/$MODEL_FILE"

curl -fL --retry 5 --retry-delay 5 -C - \
  -o "$MODEL_FILE" \
  "$MODEL_URL"

test -s "$MODEL_FILE"

magic="$(dd if="$MODEL_FILE" bs=4 count=1 2>/dev/null || true)"
if [ "$magic" != "GGUF" ]; then
  echo "[WARN] File downloaded, but GGUF magic header was not detected." >&2
fi

ls -lh "$MODEL_FILE"
OSAI_MODEL_EOF
}

# ----------------------------
# Optional firewall opening for lab/public testing.
# Default is safer: do not open ports.
# ----------------------------
open_firewall_ports_optional() {
  if [ "$FIREWALL_OPEN" != "1" ]; then
    log "Firewall opening skipped. Set FIREWALL_OPEN=1 to open common OSAI ports."
    return 0
  fi

  log "Opening common OSAI ports in firewalld if available"

  if command_exists firewall-cmd && systemctl is-active --quiet firewalld; then
    for port in 8000 8080 8001 9000 9001; do
      firewall-cmd --permanent --add-port="${port}/tcp"
    done
    firewall-cmd --reload
    firewall-cmd --list-ports
  else
    warn "firewalld not active or firewall-cmd not found. Skipping firewall changes."
  fi
}

# ----------------------------
# Keep OSAI files owned by the deploy user.
# ----------------------------
fix_permissions() {
  log "Fixing ownership and file permissions"

  chown -R "$OSAI_USER:$OSAI_USER" "$BASE_DIR"

  if [ -f "$APP_DIR/.env.storage" ]; then
    chmod 600 "$APP_DIR/.env.storage"
  fi

  if [ -f "$APP_DIR/.env.cognee" ]; then
    chmod 600 "$APP_DIR/.env.cognee"
  fi

  find "$BASE_DIR" -type d -exec chmod 755 {} \; 2>/dev/null || true
}

# ----------------------------
# Verify OS readiness without starting the OSAI app.
# ----------------------------
verify_installation() {
  log "Running verification checks"

  local failed=0
  local warned=0

  check_cmd_required() {
    if command_exists "$1"; then
      echo "OK   command: $1 -> $(command -v "$1")"
    else
      echo "MISS command: $1"
      failed=1
    fi
  }

  check_file_required() {
    if [ -e "$1" ]; then
      echo "OK   file: $1"
    else
      echo "MISS file: $1"
      failed=1
    fi
  }

  check_file_warn() {
    if [ -e "$1" ]; then
      echo "OK   file: $1"
    else
      echo "WARN file missing: $1"
      warned=1
    fi
  }

  check_dir_required() {
    if [ -d "$1" ]; then
      echo "OK   dir: $1"
    else
      echo "MISS dir: $1"
      failed=1
    fi
  }

  check_cmd_required git
  check_cmd_required curl
  check_cmd_required jq
  check_cmd_required docker

  docker --version || failed=1
  docker compose version || failed=1

  if systemctl is-active --quiet docker; then
    echo "OK   docker service active"
  else
    echo "MISS docker service active"
    failed=1
  fi

  if id "$OSAI_USER" >/dev/null 2>&1; then
    echo "OK   user exists: $OSAI_USER"
  else
    echo "MISS user: $OSAI_USER"
    failed=1
  fi

  if id -nG "$OSAI_USER" | grep -qw docker; then
    echo "OK   $OSAI_USER is in docker group"
  else
    echo "WARN $OSAI_USER is not in docker group"
    warned=1
  fi

  check_dir_required "$BASE_DIR"
  check_dir_required "$REPO_DIR"
  check_dir_required "$APP_DIR"
  check_dir_required "$MODEL_DIR"

  check_file_required "$APP_DIR/Cargo.toml"
  check_file_required "$APP_DIR/docker-compose.storage.yml"
  check_file_warn "$APP_DIR/.env.storage"
  check_file_warn "$APP_DIR/.env.cognee"

  if [ "$DOWNLOAD_MODEL" = "1" ]; then
    check_file_required "$MODEL_DIR/$MODEL_FILE"
  else
    check_file_warn "$MODEL_DIR/$MODEL_FILE"
  fi

  sudo -iu "$OSAI_USER" bash <<'OSAI_VERIFY_RUST_EOF' || failed=1
set -Eeuo pipefail

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
fi

rustc --version
cargo --version
rustup --version
OSAI_VERIFY_RUST_EOF

  if [ "$RUN_CARGO_CHECK" = "1" ]; then
    log "Running cargo metadata check"

    sudo -iu "$OSAI_USER" env APP_DIR="$APP_DIR" bash <<'OSAI_CARGO_META_EOF' && echo "OK   Cargo metadata check" || {
set -Eeuo pipefail
source "$HOME/.cargo/env"
cd "$APP_DIR"
cargo metadata --no-deps >/dev/null
OSAI_CARGO_META_EOF
      echo "MISS Cargo metadata check"
      failed=1
    }

    log "Running cargo check. This can take time on a fresh machine."

    sudo -iu "$OSAI_USER" env APP_DIR="$APP_DIR" bash <<'OSAI_CARGO_CHECK_EOF' && echo "OK   cargo check" || {
set -Eeuo pipefail
source "$HOME/.cargo/env"
cd "$APP_DIR"
cargo check
OSAI_CARGO_CHECK_EOF
      echo "MISS cargo check"
      failed=1
    }
  else
    warn "Skipping cargo check because RUN_CARGO_CHECK=$RUN_CARGO_CHECK"
    warned=1
  fi

  if [ -f "$APP_DIR/docker-compose.storage.yml" ] && [ -f "$APP_DIR/.env.storage" ]; then
    log "Checking Docker Compose storage config"

    if sudo -iu "$OSAI_USER" env APP_DIR="$APP_DIR" bash <<'OSAI_COMPOSE_CHECK_EOF'
set -Eeuo pipefail
cd "$APP_DIR"
docker compose --env-file .env.storage -f docker-compose.storage.yml config >/dev/null
OSAI_COMPOSE_CHECK_EOF
    then
      echo "OK   Docker Compose storage config"
    else
      if [ "$STRICT_COMPOSE_CHECK" = "1" ]; then
        echo "MISS Docker Compose storage config"
        failed=1
      else
        echo "WARN Docker Compose storage config failed. This may be expected until real .env values are present."
        warned=1
      fi
    fi
  fi

  echo
  echo "Verification result:"
  echo "  failed=$failed"
  echo "  warned=$warned"

  if [ "$failed" -eq 0 ]; then
    if [ "$warned" -eq 0 ]; then
      log "Stage 1 completed successfully with no warnings"
    else
      log "Stage 1 completed with warnings"
    fi
  else
    die "Stage 1 completed with failed checks"
  fi
}

# ----------------------------
# Print next steps without starting the app.
# ----------------------------
print_next_steps() {
  cat <<EOF_NEXT

============================================================
OSAI Stage 1 complete
============================================================

Deploy user:
  $OSAI_USER

Repo:
  $REPO_DIR

App:
  $APP_DIR

Model:
  $MODEL_DIR/$MODEL_FILE

Switch to the deploy user:
  sudo -iu $OSAI_USER

Then:
  cd $APP_DIR
  source ~/.cargo/env
  cargo check

Important:
  Do not run sudo from inside the $OSAI_USER shell.
  If root is needed, exit back to your normal admin user first.

Before starting infra:
  Review and edit:
    $APP_DIR/.env.storage
    $APP_DIR/.env.cognee

Useful options:
  FIREWALL_OPEN=1 sudo bash startersv.sh
  DOWNLOAD_MODEL=0 sudo bash startersv.sh
  RUN_CARGO_CHECK=0 sudo bash startersv.sh
  STRICT_COMPOSE_CHECK=1 sudo bash startersv.sh

============================================================

EOF_NEXT
}

main() {
  require_root
  detect_os
  install_base_packages
  install_docker
  create_osai_user
  install_rust_for_osai_user
  clone_repo
  prepare_env_placeholders
  download_qwen_model
  open_firewall_ports_optional
  fix_permissions
  verify_installation
  print_next_steps
}

main "$@"
