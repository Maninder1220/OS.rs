#!/usr/bin/env bash
set -Eeuxo pipefail

mkdir -p /var/log/osai
exec > >(tee -a /var/log/osai/startup.log) 2>&1

echo "OSAI starter started at $(date)"
echo "Running as user: $(whoami)"
echo "Host: $(hostname)"

# ============================================================
# ONE-SHOT BOOTSTRAP SECRETS
# ============================================================
# Fill these five values before running this script.
#
# SECURITY BOUNDARY:
#   - Values are not emitted into startup.log because xtrace is disabled here
#     and again while the values are consumed.
#   - The values still exist as plaintext in THIS SCRIPT. Keep it root-owned
#     with mode 0700 and never commit/upload it to Git.
#   - After the first successful installation, keep only a sanitized copy.
#   - Keep this same u will need it when log in enter it 3 times
#     OSAI_AGENT_TOKEN_SECRET='2865f44f20686371cdb01d0049f97124bdb97c803c257aa187888c719ebb1b73'
#
# Use single-line values. If a value contains a single quote, represent it as:
#   'part1'\''part2'
# ============================================================
set +x
COGNEE_API_URL_SECRET='https://your-cognee-tenant-url-fron-cognee-api-page.aws.cognee.ai'
COGNEE_API_KEY_SECRET='your88cognee88api88key88from88cognee88api88page'
COGNEE_TENANT_ID_SECRET='your88cognee88tenant88id88from88cognee88api88page'
COGNEE_USER_ID_SECRET='your88cognee88user88id88from88cognee88api88page'
OSAI_AGENT_TOKEN_SECRET='2865f44f20686371cdb01d0049f97124bdb97c803c257aa187888c719ebb1b73'
set -x

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
#   7. Replace active .env files from configurable source files, with backups.
#   8. Download the Qwen GGUF model into the OSAI app models directory.
#   9. Fix ownership and run readiness checks.
#  10. Build and execute the Rust OS-readiness setter as the osai user.
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

MODEL_FILE="${MODEL_FILE:-Qwen3-1.7B-Q8_0.gguf}"
MODEL_URL="${MODEL_URL:-https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q8_0.gguf?download=true}"
MODEL_SHA256="${MODEL_SHA256:-061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a}"
MODEL_DIR="${MODEL_DIR:-$APP_DIR/models}"

# Compatibility filename used by older OSAI/Rust-setter and Compose logic.
# This is a symbolic link to MODEL_FILE, not a second copy of the model.
LEGACY_MODEL_FILE="${LEGACY_MODEL_FILE:-Qwen3-4B-Q4_K_M.gguf}"

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

# Existing .env files are preserved. They are copied from the repo examples
# only when missing; the encrypted-credential injector changes only five keys.
ENV_STORAGE_SOURCE="${ENV_STORAGE_SOURCE:-$APP_DIR/.env.storage.example}"
ENV_COGNEE_SOURCE="${ENV_COGNEE_SOURCE:-$APP_DIR/.env.cognee.example}"

# Rust OS-readiness setter built and executed after Stage 1 succeeds.
RUN_RUST_SETTER="${RUN_RUST_SETTER:-1}"
RUST_SETTER_DIR="${RUST_SETTER_DIR:-$REPO_DIR/get-osai-os-ready/Layers}"
RUST_SETTER_BINARY="${RUST_SETTER_BINARY:-get-osai-os-ready}"

# Encrypted systemd credential paths.
CREDENTIAL_DIR="${CREDENTIAL_DIR:-/etc/credstore.encrypted}"
CREDENTIAL_INJECTOR="${CREDENTIAL_INJECTOR:-/usr/local/sbin/osai-inject-env}"
CREDENTIAL_UNIT="${CREDENTIAL_UNIT:-/etc/systemd/system/osai-secrets.service}"
CREDENTIAL_SERVICE="${CREDENTIAL_SERVICE:-osai-secrets.service}"

# OSAI all-in-one background service.
OSAI_ALL_BINARY="${OSAI_ALL_BINARY:-$APP_DIR/target/release/osai-all}"
OSAI_AGENT_UNIT="${OSAI_AGENT_UNIT:-/etc/systemd/system/osai-agent.service}"
OSAI_AGENT_SERVICE="${OSAI_AGENT_SERVICE:-osai-agent.service}"

# Values printed at successful completion for the operator's local tunnel.
GCP_PROJECT_ID="${GCP_PROJECT_ID:-project-e3bbb75a-9976-4b98-8df}"
GCP_INSTANCE_NAME="${GCP_INSTANCE_NAME:-alma-dev-vm}"
GCP_ZONE="${GCP_ZONE:-us-central1-a}"

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
# Replace active env files from configurable source files.
# Existing active files are backed up before replacement.
# ----------------------------
prepare_env_placeholders() {
  log "Preparing OSAI environment files without replacing existing configuration"

  [ -d "$APP_DIR" ] || die "APP_DIR missing: $APP_DIR"

  if [ ! -f "$APP_DIR/.env.storage" ]; then
    [ -f "$ENV_STORAGE_SOURCE" ] ||
      die "Missing .env.storage and source file: $ENV_STORAGE_SOURCE"
    install -o "$OSAI_USER" -g "$OSAI_USER" -m 0600 \
      "$ENV_STORAGE_SOURCE" "$APP_DIR/.env.storage"
    log "Created $APP_DIR/.env.storage"
  else
    log "Keeping existing $APP_DIR/.env.storage"
  fi

  if [ ! -f "$APP_DIR/.env.cognee" ]; then
    [ -f "$ENV_COGNEE_SOURCE" ] ||
      die "Missing .env.cognee and source file: $ENV_COGNEE_SOURCE"
    install -o "$OSAI_USER" -g "$OSAI_USER" -m 0600 \
      "$ENV_COGNEE_SOURCE" "$APP_DIR/.env.cognee"
    log "Created $APP_DIR/.env.cognee"
  else
    log "Keeping existing $APP_DIR/.env.cognee"
  fi

  chown "$OSAI_USER:$OSAI_USER" \
    "$APP_DIR/.env.storage" "$APP_DIR/.env.cognee"
  chmod 0600 "$APP_DIR/.env.storage" "$APP_DIR/.env.cognee"
}

# ----------------------------
# Install and activate encrypted systemd credentials in the same run.
# The five plaintext values are consumed only while xtrace is disabled.
# ----------------------------
install_and_activate_systemd_credentials() {
  log "Installing one-shot encrypted systemd credential flow"

  [ "$(ps -p 1 -o comm= | xargs)" = "systemd" ] ||
    die "PID 1 is not systemd; encrypted service credentials cannot be activated"

  command_exists systemd-creds ||
    die "systemd-creds is unavailable. Use Ubuntu 24.04+ or AlmaLinux 9/10 with systemd-creds installed."

  command_exists systemd-analyze || die "systemd-analyze is unavailable"

  local systemd_version
  systemd_version="$(systemd-analyze --version | awk 'NR == 1 {print $2}')"
  [[ "$systemd_version" =~ ^[0-9]+$ ]] || die "Could not determine systemd version"
  [ "$systemd_version" -ge 250 ] ||
    die "systemd $systemd_version is too old; systemd-creds requires version 250 or newer"

  install -d -o root -g root -m 0700 "$CREDENTIAL_DIR"

  cat >"$CREDENTIAL_INJECTOR" <<'OSAI_INJECTOR_EOF'
#!/usr/bin/env bash
set -Eeuo pipefail

# Never enable xtrace in this script: it processes decrypted credentials.
OSAI_USER="${OSAI_USER:-osai}"
APP_DIR="${APP_DIR:-/opt/osai/OS.rs/osai-agent}"
COGNEE_ENV="$APP_DIR/.env.cognee"
STORAGE_ENV="$APP_DIR/.env.storage"

die() {
  printf '[ERROR] %s\n' "$*" >&2
  exit 1
}

read_credential() {
  local name="$1"
  local path="${CREDENTIALS_DIRECTORY:?CREDENTIALS_DIRECTORY is unavailable}/$name"
  local value

  [ -r "$path" ] || die "Credential unavailable: $name"
  value="$(<"$path")"
  [ -n "$value" ] || die "Credential is empty: $name"
  [[ "$value" != *$'\n'* && "$value" != *$'\r'* ]] ||
    die "Credential must be a single line: $name"
  printf '%s' "$value"
}

upsert_env_key() {
  local target_file="$1"
  local target_key="$2"
  local target_value="$3"
  local temporary_file line
  local written=0

  [ -f "$target_file" ] || die "Environment file missing: $target_file"
  temporary_file="$(mktemp "${target_file}.tmp.XXXXXX")"
  chmod 0600 "$temporary_file"

  while IFS= read -r line || [ -n "$line" ]; do
    if [[ "$line" == "${target_key}="* ]]; then
      if [ "$written" -eq 0 ]; then
        printf '%s=%s\n' "$target_key" "$target_value" >>"$temporary_file"
        written=1
      fi
      continue
    fi
    printf '%s\n' "$line" >>"$temporary_file"
  done <"$target_file"

  if [ "$written" -eq 0 ]; then
    printf '%s=%s\n' "$target_key" "$target_value" >>"$temporary_file"
  fi

  chown "$OSAI_USER:$OSAI_USER" "$temporary_file"
  chmod 0600 "$temporary_file"
  mv -f -- "$temporary_file" "$target_file"
}

main() {
  local cognee_api_url cognee_api_key cognee_tenant_id
  local cognee_user_id osai_agent_token

  cognee_api_url="$(read_credential COGNEE_API_URL)"
  cognee_api_key="$(read_credential COGNEE_API_KEY)"
  cognee_tenant_id="$(read_credential COGNEE_TENANT_ID)"
  cognee_user_id="$(read_credential COGNEE_USER_ID)"
  osai_agent_token="$(read_credential OSAI_AGENT_TOKEN)"

  upsert_env_key "$COGNEE_ENV" COGNEE_API_URL "$cognee_api_url"
  upsert_env_key "$COGNEE_ENV" COGNEE_API_KEY "$cognee_api_key"
  upsert_env_key "$COGNEE_ENV" COGNEE_TENANT_ID "$cognee_tenant_id"
  upsert_env_key "$COGNEE_ENV" COGNEE_USER_ID "$cognee_user_id"
  upsert_env_key "$STORAGE_ENV" OSAI_AGENT_TOKEN "$osai_agent_token"

  unset cognee_api_url cognee_api_key cognee_tenant_id
  unset cognee_user_id osai_agent_token

  printf 'OSAI protected environment values updated successfully\n'
}

main "$@"
OSAI_INJECTOR_EOF

  chown root:root "$CREDENTIAL_INJECTOR"
  chmod 0700 "$CREDENTIAL_INJECTOR"

  cat >"$CREDENTIAL_UNIT" <<OSAI_CREDENTIAL_UNIT_EOF
[Unit]
Description=Inject encrypted OSAI credentials into application environment files
After=local-fs.target
ConditionPathExists=$APP_DIR/.env.cognee
ConditionPathExists=$APP_DIR/.env.storage

[Service]
Type=oneshot
User=root
Group=root
UMask=0077

LoadCredentialEncrypted=COGNEE_API_URL:$CREDENTIAL_DIR/COGNEE_API_URL.cred
LoadCredentialEncrypted=COGNEE_API_KEY:$CREDENTIAL_DIR/COGNEE_API_KEY.cred
LoadCredentialEncrypted=COGNEE_TENANT_ID:$CREDENTIAL_DIR/COGNEE_TENANT_ID.cred
LoadCredentialEncrypted=COGNEE_USER_ID:$CREDENTIAL_DIR/COGNEE_USER_ID.cred
LoadCredentialEncrypted=OSAI_AGENT_TOKEN:$CREDENTIAL_DIR/OSAI_AGENT_TOKEN.cred

Environment=OSAI_USER=$OSAI_USER
Environment=APP_DIR=$APP_DIR
ExecStart=$CREDENTIAL_INJECTOR
RemainAfterExit=yes

NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
PrivateMounts=yes
ProtectHome=yes
ProtectSystem=strict
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
LockPersonality=yes
ReadWritePaths=$APP_DIR

[Install]
WantedBy=multi-user.target
OSAI_CREDENTIAL_UNIT_EOF

  chown root:root "$CREDENTIAL_UNIT"
  chmod 0644 "$CREDENTIAL_UNIT"

  if command_exists restorecon; then
    restorecon -F "$CREDENTIAL_INJECTOR" "$CREDENTIAL_UNIT" "$CREDENTIAL_DIR" \
      2>/dev/null || true
  fi

  # Stop tracing before validating, expanding, piping, or unsetting secrets.
  local xtrace_was_enabled=0
  case "$-" in
    *x*) xtrace_was_enabled=1 ;;
  esac
  set +x

  validate_secret() {
    local name="$1"
    local value="$2"
    case "$value" in
      ''|CHANGE_ME_*)
        printf '[ERROR] Set a real value for %s in the bootstrap secret section\n' "$name" >&2
        return 1
        ;;
    esac
    [[ "$value" != *$'\n'* && "$value" != *$'\r'* ]] || {
      printf '[ERROR] %s must be a single-line value\n' "$name" >&2
      return 1
    }
  }

  validate_secret COGNEE_API_URL "$COGNEE_API_URL_SECRET"
  validate_secret COGNEE_API_KEY "$COGNEE_API_KEY_SECRET"
  validate_secret COGNEE_TENANT_ID "$COGNEE_TENANT_ID_SECRET"
  validate_secret COGNEE_USER_ID "$COGNEE_USER_ID_SECRET"
  validate_secret OSAI_AGENT_TOKEN "$OSAI_AGENT_TOKEN_SECRET"

  local temporary_dir
  temporary_dir="$(mktemp -d "$CREDENTIAL_DIR/.bootstrap.XXXXXX")"
  chmod 0700 "$temporary_dir"

  cleanup_credential_temp() {
    rm -rf -- "$temporary_dir"
  }
  trap cleanup_credential_temp EXIT INT TERM

  encrypt_credential() {
    local name="$1"
    local value="$2"
    local temporary_output="$temporary_dir/$name.cred"

    printf '%s' "$value" |
      systemd-creds encrypt --name="$name" - "$temporary_output" >/dev/null

    install -o root -g root -m 0600 \
      "$temporary_output" "$CREDENTIAL_DIR/$name.cred"
  }

  encrypt_credential COGNEE_API_URL "$COGNEE_API_URL_SECRET"
  encrypt_credential COGNEE_API_KEY "$COGNEE_API_KEY_SECRET"
  encrypt_credential COGNEE_TENANT_ID "$COGNEE_TENANT_ID_SECRET"
  encrypt_credential COGNEE_USER_ID "$COGNEE_USER_ID_SECRET"
  encrypt_credential OSAI_AGENT_TOKEN "$OSAI_AGENT_TOKEN_SECRET"

  unset COGNEE_API_URL_SECRET COGNEE_API_KEY_SECRET
  unset COGNEE_TENANT_ID_SECRET COGNEE_USER_ID_SECRET
  unset OSAI_AGENT_TOKEN_SECRET

  cleanup_credential_temp
  trap - EXIT INT TERM

  if [ "$xtrace_was_enabled" -eq 1 ]; then
    set -x
  fi

  systemctl daemon-reload
  systemd-analyze verify "$CREDENTIAL_UNIT"
  systemctl enable --now "$CREDENTIAL_SERVICE"
  systemctl is-active --quiet "$CREDENTIAL_SERVICE" ||
    die "$CREDENTIAL_SERVICE failed to start"

  log "Encrypted OSAI credentials created, injected, and enabled for future boots"
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
    MODEL_SHA256="$MODEL_SHA256" \
    bash <<'OSAI_MODEL_EOF'
set -Eeuo pipefail

mkdir -p "$MODEL_DIR"
cd "$MODEL_DIR"

if [ -s "$MODEL_FILE" ]; then
  echo "[INFO] Model already exists: $MODEL_DIR/$MODEL_FILE"
  echo "[INFO] Validating the existing model before continuing"
else
  echo "[INFO] Downloading model. If interrupted, rerun this script to resume."
  echo "[INFO] Target: $MODEL_DIR/$MODEL_FILE"

  curl -fL --retry 5 --retry-delay 5 -C - \
    -o "$MODEL_FILE" \
    "$MODEL_URL"
fi

test -s "$MODEL_FILE"

magic="$(dd if="$MODEL_FILE" bs=4 count=1 2>/dev/null || true)"
if [ "$magic" != "GGUF" ]; then
  echo "[ERROR] Downloaded file does not have a GGUF header: $MODEL_FILE" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1 && [ -n "${MODEL_SHA256:-}" ]; then
  actual_sha256="$(sha256sum "$MODEL_FILE" | awk '{print $1}')"
  if [ "$actual_sha256" != "$MODEL_SHA256" ]; then
    echo "[ERROR] SHA-256 mismatch for $MODEL_FILE" >&2
    echo "[ERROR] Expected: $MODEL_SHA256" >&2
    echo "[ERROR] Actual:   $actual_sha256" >&2
    exit 1
  fi
  echo "[INFO] SHA-256 verified: $actual_sha256"
fi

ls -lh "$MODEL_FILE"
OSAI_MODEL_EOF
}

# ----------------------------
# Keep old hard-coded OSAI paths working without storing a second model copy.
# The Rust readiness setter currently checks Qwen3-4B-Q4_K_M.gguf.
# ----------------------------
prepare_model_compatibility_alias() {
  log "Preparing Qwen model compatibility filename"

  local model_path="$MODEL_DIR/$MODEL_FILE"
  local legacy_path="$MODEL_DIR/$LEGACY_MODEL_FILE"

  [ -s "$model_path" ] ||
    die "Model missing: $model_path"

  if [ "$MODEL_FILE" = "$LEGACY_MODEL_FILE" ]; then
    log "Model filename already matches the compatibility filename"
    return 0
  fi

  if [ -L "$legacy_path" ]; then
    if [ "$(readlink "$legacy_path")" = "$MODEL_FILE" ]; then
      log "Compatibility symlink already correct: $legacy_path -> $MODEL_FILE"
    else
      warn "Replacing incorrect model symlink: $legacy_path"
      rm -f -- "$legacy_path"
      ln -s -- "$MODEL_FILE" "$legacy_path"
    fi
  elif [ -e "$legacy_path" ]; then
    local backup_path
    backup_path="${legacy_path}.backup.$(date '+%Y%m%d%H%M%S')"
    warn "A regular file already exists at the legacy model path"
    warn "Moving it to: $backup_path"
    mv -- "$legacy_path" "$backup_path"
    ln -s -- "$MODEL_FILE" "$legacy_path"
  else
    ln -s -- "$MODEL_FILE" "$legacy_path"
  fi

  chown -h "$OSAI_USER:$OSAI_USER" "$legacy_path"

  [ -s "$legacy_path" ] ||
    die "Compatibility model path is not readable: $legacy_path"

  echo "OK   model: $model_path"
  echo "OK   compatibility alias: $legacy_path -> $(readlink "$legacy_path")"
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
    check_file_required "$MODEL_DIR/$LEGACY_MODEL_FILE"

    if [ -L "$MODEL_DIR/$LEGACY_MODEL_FILE" ] &&
       [ "$(readlink "$MODEL_DIR/$LEGACY_MODEL_FILE")" = "$MODEL_FILE" ]; then
      echo "OK   legacy model alias -> $MODEL_FILE"
    else
      echo "MISS legacy model alias: $MODEL_DIR/$LEGACY_MODEL_FILE"
      failed=1
    fi
  else
    check_file_warn "$MODEL_DIR/$MODEL_FILE"
    check_file_warn "$MODEL_DIR/$LEGACY_MODEL_FILE"
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
# Build and run the Rust OS-readiness setter as the dedicated osai user.
# This is intentionally executed only after all Stage 1 verification succeeds.
# ----------------------------
run_rust_setter() {
  if [ "$RUN_RUST_SETTER" != "1" ]; then
    log "Skipping Rust setter because RUN_RUST_SETTER=$RUN_RUST_SETTER"
    return 0
  fi

  log "Building and running Rust setter"

  [ -d "$RUST_SETTER_DIR" ] || die "Rust setter directory missing: $RUST_SETTER_DIR"
  [ -f "$RUST_SETTER_DIR/Cargo.toml" ] || die "Cargo.toml missing in: $RUST_SETTER_DIR"

  chown -R "$OSAI_USER:$OSAI_USER" "$RUST_SETTER_DIR"

  sudo -iu "$OSAI_USER" env \
    RUST_SETTER_DIR="$RUST_SETTER_DIR" \
    RUST_SETTER_BINARY="$RUST_SETTER_BINARY" \
    bash <<'OSAI_RUST_SETTER_EOF'
set -Eeuo pipefail

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
else
  export PATH="$HOME/.cargo/bin:$PATH"
fi

cd "$RUST_SETTER_DIR"

echo "[INFO] Running cargo check in $PWD"
cargo check

echo "[INFO] Building optimized release binary"
cargo build --release

setter_path="./target/release/$RUST_SETTER_BINARY"
test -x "$setter_path" || {
  echo "[ERROR] Expected executable not found: $RUST_SETTER_DIR/$setter_path" >&2
  exit 1
}

echo "[INFO] Starting Rust setter: $setter_path"
"$setter_path"
OSAI_RUST_SETTER_EOF

  log "Rust setter completed successfully"
}

# ----------------------------
# Ensure the all-in-one OSAI release executable exists.
# get-osai-os-ready is expected to prepare it; this build is a safe fallback.
# ----------------------------
ensure_osai_all_binary() {
  if [ -x "$OSAI_ALL_BINARY" ]; then
    log "OSAI all-in-one binary is ready: $OSAI_ALL_BINARY"
    return 0
  fi

  log "osai-all was not found after readiness setup; building it now"

  [ -f "$APP_DIR/Cargo.toml" ] || die "Cargo.toml missing: $APP_DIR/Cargo.toml"

  sudo -iu "$OSAI_USER" env APP_DIR="$APP_DIR" bash <<'OSAI_ALL_BUILD_EOF'
set -Eeuo pipefail

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
else
  export PATH="$HOME/.cargo/bin:$PATH"
fi

cd "$APP_DIR"
cargo build --release --bin osai-all
OSAI_ALL_BUILD_EOF

  [ -x "$OSAI_ALL_BINARY" ] ||
    die "Expected OSAI executable was not produced: $OSAI_ALL_BINARY"
}

# ----------------------------
# Run osai-all in the background under systemd.
# systemd owns restart behavior and sends stdout/stderr to journald.
# ----------------------------
install_and_start_osai_agent_service() {
  log "Installing and starting $OSAI_AGENT_SERVICE"

  ensure_osai_all_binary

  cat >"$OSAI_AGENT_UNIT" <<EOF_OSAI_AGENT_UNIT
[Unit]
Description=OSAI all-in-one agent
Requires=docker.service $CREDENTIAL_SERVICE
After=network-online.target docker.service $CREDENTIAL_SERVICE
Wants=network-online.target

[Service]
Type=simple
User=$OSAI_USER
Group=$OSAI_USER
WorkingDirectory=$APP_DIR
Environment=HOME=/home/$OSAI_USER
ExecStart=$OSAI_ALL_BINARY
Restart=on-failure
RestartSec=5s
TimeoutStopSec=30s
KillSignal=SIGTERM
StandardOutput=journal
StandardError=journal
SyslogIdentifier=osai-all
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
EOF_OSAI_AGENT_UNIT

  chown root:root "$OSAI_AGENT_UNIT"
  chmod 0644 "$OSAI_AGENT_UNIT"

  if command_exists restorecon; then
    restorecon -F "$OSAI_AGENT_UNIT" 2>/dev/null || true
  fi

  systemctl daemon-reload
  systemd-analyze verify "$OSAI_AGENT_UNIT"
  systemctl enable --now "$OSAI_AGENT_SERVICE"

  # Give a fast-failing process enough time to report its startup error.
  sleep 3

  if ! systemctl is-active --quiet "$OSAI_AGENT_SERVICE"; then
    journalctl -u "$OSAI_AGENT_SERVICE" -n 100 --no-pager >&2 || true
    die "$OSAI_AGENT_SERVICE failed to remain active"
  fi

  log "$OSAI_AGENT_SERVICE is active"
}

mark_startup_complete() {
  mkdir -p /var/lib/osai
  printf 'startup completed at %s\n' "$(date)" > /var/lib/osai/startup.done
  chmod 0644 /var/lib/osai/startup.done
}

# ----------------------------
# Print the completion summary only after the setter and agent have succeeded.
# ----------------------------
print_completion_summary() {
  cat <<EOF_NEXT

============================================================
OSAI installation and startup complete
============================================================

Deploy user:
  $OSAI_USER

Repo:
  $REPO_DIR

App:
  $APP_DIR

Readiness setter:
  $RUST_SETTER_DIR/target/release/$RUST_SETTER_BINARY

OSAI agent:
  Binary:  $OSAI_ALL_BINARY
  Service: $OSAI_AGENT_SERVICE
  Status:  active

Model:
  Real file:           $MODEL_DIR/$MODEL_FILE
  Compatibility path:  $MODEL_DIR/$LEGACY_MODEL_FILE

Encrypted credentials:
  Store:   $CREDENTIAL_DIR
  Service: $CREDENTIAL_SERVICE

Copy/paste on the VM to follow OSAI logs live:
  sudo journalctl -fu $OSAI_AGENT_SERVICE --no-pager -n all

Copy/paste locally to follow OSAI logs through IAP:
  gcloud compute ssh $GCP_INSTANCE_NAME \\
    --project=$GCP_PROJECT_ID \\
    --zone=$GCP_ZONE \\
    --tunnel-through-iap \\
    --command='sudo journalctl -fu $OSAI_AGENT_SERVICE --no-pager -n all'

Copy/paste locally to open the application tunnels:
  gcloud compute ssh $GCP_INSTANCE_NAME \\
    --project=$GCP_PROJECT_ID \\
    --zone=$GCP_ZONE \\
    --tunnel-through-iap \\
    -- -N \\
    -L 8000:127.0.0.1:8000 \\
    -L 8001:127.0.0.1:8001 \\
    -L 8080:127.0.0.1:8080 \\
    -L 9001:127.0.0.1:9001

Local endpoints while that tunnel is open:
  http://127.0.0.1:8000
  http://127.0.0.1:8001
  http://127.0.0.1:8080
  http://127.0.0.1:9001

Completion marker:
  /var/lib/osai/startup.done

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
  install_and_activate_systemd_credentials
  download_qwen_model
  prepare_model_compatibility_alias
  open_firewall_ports_optional
  fix_permissions
  verify_installation
  run_rust_setter
  install_and_start_osai_agent_service
  mark_startup_complete
  print_completion_summary
}

main "$@"

echo "OSAI starter finished at $(date)"