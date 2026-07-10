# OSAI Agent - 2865f44f20686371cdb01d0049f97124bdb97c803c257aa187888c719ebb1b73

OSAI Agent is a **Rust-first local Linux and DevOps operations assistant**.

It scans a Linux machine, stores exact system facts, archives raw evidence, builds searchable AI memory, and answers operator questions through a local Qwen model running behind llama.cpp.

The core design rule is:

```text
Rust is the source of truth.
Qwen is only the natural-language reasoning layer.
```

That means OSAI should not send the whole server state to Qwen. Rust first detects the user intent, builds a focused FactPack, and sends Qwen only the facts needed for that question.

Example:

```text
"what my cpu doing"      -> CPU facts only
"what about ram"         -> memory facts only
"update on service"      -> service/process/database facts only
"whats the update"       -> compact server overview
```

---

## Architecture

```text
Browser dashboard
  -> osai-agent Rust API
  -> AskPlan intent detection
  -> FactPack focused evidence
  -> optional Cognee memory recall
  -> optional llama.cpp/Qwen answer
```

Runtime components:

```text
Rust binaries:
  osai-agent             Dashboard, scanner, API, guarded actions
  osai-all               One-command local supervisor
  osai-storage-worker    PostgreSQL + RustFS persistence worker
  osai-cognee-ingest     Pushes pending memory rows to Cognee
  osai-ask               CLI ask path for Qwen/Cognee testing

Docker services:
  postgres               OSAI operational DB + Cognee DB + pgvector
  rustfs                 S3-compatible raw evidence store
  rustfs-init            Creates the osai-agent bucket
  llama                  llama.cpp server running Qwen GGUF
  cognee                 Cognee REST API memory/retrieval server
```

Data roles:

```text
PostgreSQL = exact facts, scan metadata, findings, outbox state
RustFS     = raw JSON snapshots and Markdown evidence
Cognee     = searchable memory/retrieval layer
Qwen       = natural-language answer layer
Rust       = scanner, planner, guardrails, severity, command suggestions
```

---

## What you need handy before installing

Keep these ready before deployment.

### Access and repo

```text
GitHub repository:
  https://github.com/Maninder1220/OS.rs.git

Branches:
  main    = source build/development path
  binary  = binary-only deployment path, if you maintain that branch
```

### Local machine requirements

```text
Linux host:
  Ubuntu, Debian, RHEL, AlmaLinux, Rocky, Fedora, or WSL2 for development

Required tools:
  git
  curl
  jq
  docker
  docker compose plugin
  rustup / cargo / rustfmt
```

### Model file

Default model expected by this project:

```text
models/Qwen3-4B-Q4_K_M.gguf
```

The model is intentionally not committed to Git because it is large.

### Cognee details

For local Cognee Docker mode, defaults from `.env.cognee.example` are enough for testing.

For Cognee Cloud or manually deployed Cognee API, keep these handy:

```text
COGNEE_API_URL
COGNEE_API_PREFIX
COGNEE_API_KEY
COGNEE_TENANT_ID
COGNEE_USER_ID
COGNEE_DATASET
```

If your Cognee deployment uses explicit users/tenants, also keep:

```text
tenant name
tenant id
user id
username / email
password or API key
role name, if access control is enabled
```

### Database and object-store details

OSAI operational database:

```text
OSAI_POSTGRES_DSN=postgresql://osai:osai_password@127.0.0.1:5432/osai_agent
```

Cognee local database:

```text
DB_PROVIDER=postgres
DB_HOST=postgres
DB_PORT=5432
DB_USERNAME=cognee
DB_PASSWORD=cognee_password
DB_NAME=cognee_db
```

RustFS object store:

```text
OBJECT_STORE_ENDPOINT=127.0.0.1:9000
OBJECT_STORE_ACCESS_KEY=rustfsadmin
OBJECT_STORE_SECRET_KEY=rustfsadmin
OBJECT_STORE_BUCKET=osai-agent
OBJECT_STORE_SECURE=false
OBJECT_STORE_REGION=us-east-1
```

Dashboard token:

```text
OSAI_AGENT_TOKEN=
```

Leave this empty for local development. Set it only when exposing the dashboard outside localhost or when you intentionally want token-protected API access.

---

## Used dependencies

### System dependencies

```text
git                  Clone and update the project
curl                 Download model files and test APIs
jq                   Pretty-print JSON API responses
docker               Run PostgreSQL, RustFS, Cognee, and llama.cpp
docker compose       Start the full local support stack
rustup/cargo         Build Rust binaries
rustfmt/cargo fmt    Format Rust code before build/commit
aws-cli              Optional RustFS/S3 verification
```

### Rust project dependencies

The Rust dependency list lives in:

```text
Cargo.toml
Cargo.lock
```

Cargo downloads Rust crates and builds the binaries.

Main Rust responsibilities:

```text
axum / tokio         Web API and async runtime
serde / serde_json   JSON request/response and snapshot structures
reqwest              HTTP calls to Cognee and llama.cpp
sqlx                 PostgreSQL persistence
tracing              structured logs
```

### Runtime service dependencies

```text
PostgreSQL + pgvector
RustFS
llama.cpp server
Qwen3 GGUF model
Cognee REST API
```

### AI/model dependencies

```text
Qwen3-4B-Q4_K_M.gguf
llama.cpp OpenAI-compatible HTTP endpoint
Cognee REST recall/remember endpoint
```

---

## Installation from source branch

Start with Git clone:

```bash
git clone https://github.com/Maninder1220/OS.rs.git
cd OS.rs
git checkout main
```

Find the Rust project folder:

```bash
find . -maxdepth 3 -name Cargo.toml -print
```

Go into the folder that contains `Cargo.toml`.

Example:

```bash
cd osai-agent
```

If `Cargo.toml` is already in the repository root, stay in the root.

---

## Binary-only deployment branch

Use this only when you maintain a branch that already contains compiled binaries or packaging output.

```bash
git clone --branch binary https://github.com/Maninder1220/OS.rs.git OS.rs-binary
cd OS.rs-binary
```

If your binary branch has a different name, replace `binary` with the real branch name.

Expected binary deployment idea:

```text
No Rust compile needed on the target machine.
Use shipped target/release binaries, packaged tar, RPM, or systemd units.
Still keep Docker Compose services available for PostgreSQL, RustFS, Cognee, and llama.cpp unless they run elsewhere.
```

Typical binary run:

```bash
chmod +x target/release/osai-*
RUST_LOG=info ./target/release/osai-all
```

---

## Configure environment files

Copy examples:

```bash
cp .env.storage.example .env.storage
cp .env.cognee.example .env.cognee
```

Edit local values:

```bash
nano .env.storage
nano .env.cognee
```

For local development, keep:

```env
OSAI_AGENT_TOKEN=
REQUIRE_AUTHENTICATION=false
ENABLE_BACKEND_ACCESS_CONTROL=false
```

For production or remote dashboard exposure, set a long random token:

```bash
export OSAI_AGENT_TOKEN='change-me-long-random-token'
```

---

## Download Qwen GGUF model

Create the model folder:

```bash
mkdir -p models
```

Download the default model:

```bash
curl -L \
  -o models/Qwen3-4B-Q4_K_M.gguf \
  https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf
```

Verify:

```bash
ls -lh models/Qwen3-4B-Q4_K_M.gguf
```

Expected size is roughly a few GB. If the file is tiny, the download failed or was interrupted.

If your file is named differently, either rename it:

```bash
mv models/qwen3-4B-q4.gguf models/Qwen3-4B-Q4_K_M.gguf
```

or set the model filename when starting Compose:

```bash
OSAI_GGUF_MODEL_FILE=qwen3-4B-q4.gguf docker compose -f docker-compose.storage.yml up -d --build
```

---

## Start local support stack

Host-mounted model mode:

```bash
docker compose -f docker-compose.storage.yml up -d --build
```

This starts:

```text
osai-postgres
osai-rustfs
osai-rustfs-init
osai-llama
osai-cognee
```

Check containers:

```bash
docker compose -f docker-compose.storage.yml ps
```

Check logs:

```bash
docker logs osai-postgres --tail 50
docker logs osai-rustfs --tail 50
docker logs osai-llama --tail 50
docker logs osai-cognee --tail 50
```

---

## Build Rust binaries

Run this after cloning or after applying code changes:

```bash
cargo fmt
cargo check
cargo build --release
```

Where these are used:

```text
cargo fmt
  Formats Rust source code. Run this after patches or edits and before commit/build.

cargo check
  Fast compile/type check. Use it before release build to catch Rust errors quickly.

cargo build --release
  Builds optimized production binaries under target/release/.
```

Release binaries:

```text
target/release/osai-agent
target/release/osai-all
target/release/osai-storage-worker
target/release/osai-cognee-ingest
target/release/osai-ask
```

---

## Run full local OSAI

Recommended local run:

```bash
RUST_LOG=info ./target/release/osai-all
```

What it does:

```text
1. Starts Docker support stack
2. Ensures RustFS bucket exists
3. Starts osai-agent
4. Starts osai-storage-worker
5. Starts osai-cognee-ingest
```

Open dashboard:

```text
http://127.0.0.1:8000
```

---

## Manual RustFS bucket init command

Use this command when:

```text
storage worker logs show NoSuchBucket
RustFS was recreated
Docker volume was deleted
osai-rustfs-init exited too early
you want to verify the bucket exists before testing Ask OSAI
```

Command:

```bash
docker compose -f docker-compose.storage.yml run --rm --no-deps rustfs-init
```

Expected output:

```text
waiting for RustFS S3 API
ensuring bucket: osai-agent
Bucket created successfully
RustFS buckets:
osai-agent/
```

Verify with AWS CLI, optional:

```bash
export AWS_ACCESS_KEY_ID=rustfsadmin
export AWS_SECRET_ACCESS_KEY=rustfsadmin
export AWS_DEFAULT_REGION=us-east-1
export AWS_EC2_METADATA_DISABLED=true

aws --endpoint-url http://127.0.0.1:9000 s3 ls
aws --endpoint-url http://127.0.0.1:9000 s3 ls s3://osai-agent/ --recursive
```

---

## API checks

Health:

```bash
curl http://127.0.0.1:8000/api/health | jq
```

Snapshot:

```bash
curl http://127.0.0.1:8000/api/snapshot | jq
```

Ask OSAI without Qwen, useful for testing intent path:

```bash
curl -X POST http://127.0.0.1:8000/api/ask \
  -H 'Content-Type: application/json' \
  -d '{"question":"what my cpu doing","use_ai":false}' | jq
```

Ask OSAI with Qwen:

```bash
curl -X POST http://127.0.0.1:8000/api/ask \
  -H 'Content-Type: application/json' \
  -d '{"question":"what is update on service","use_ai":true}' | jq
```

If token auth is enabled:

```bash
curl -X POST http://127.0.0.1:8000/api/ask \
  -H "X-OSAI-Token: $OSAI_AGENT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"question":"what about ram","use_ai":false}' | jq
```

---

## Test AskPlan + FactPack intent

Use these dashboard questions:

```text
what my cpu doing
what about ram
what is update on service
whats the update
```

Expected behavior:

```text
CPU question      -> CPU intent, CPU facts only
RAM question      -> Memory intent, memory facts only
Service question  -> Services/process/database facts only
General update    -> compact server overview
```

The UI should show:

```text
Detected intent
Data sent to AI
AI used
Mode/source
Manual checks
```

---

## Common issues

### 1. Qwen3 GGUF model missing or interrupted download

Symptom:

```text
llama container starts but model load fails
/api/ask returns llama.cpp/Qwen error
models/Qwen3-4B-Q4_K_M.gguf does not exist
file size is too small
```

Fix:

```bash
mkdir -p models
rm -f models/Qwen3-4B-Q4_K_M.gguf

curl -L \
  -o models/Qwen3-4B-Q4_K_M.gguf \
  https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf

ls -lh models/Qwen3-4B-Q4_K_M.gguf
docker compose -f docker-compose.storage.yml restart llama
```

### 2. After git clone, models folder is empty

This is expected. GGUF files are ignored by Git.

Fix:

```bash
cd OS.rs
find . -maxdepth 3 -type d -name models -print
mkdir -p models
curl -L \
  -o models/Qwen3-4B-Q4_K_M.gguf \
  https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf
```

If your Rust project is inside `osai-agent/`, use:

```bash
cd OS.rs/osai-agent
mkdir -p models
curl -L \
  -o models/Qwen3-4B-Q4_K_M.gguf \
  https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf
```

### 3. RustFS says NoSuchBucket

Symptom:

```text
Server returned non-2xx status code: 404 Not Found
<Code>NoSuchBucket</Code>
```

Cause:

```text
osai-storage-worker tried to upload snapshots before the osai-agent bucket existed.
```

Fix:

```bash
docker compose -f docker-compose.storage.yml run --rm --no-deps rustfs-init
docker compose -f docker-compose.storage.yml restart rustfs
RUST_LOG=info ./target/release/osai-all
```

Also verify these match:

```env
OBJECT_STORE_BUCKET=osai-agent
OBJECT_STORE_ENDPOINT=127.0.0.1:9000
OBJECT_STORE_ACCESS_KEY=rustfsadmin
OBJECT_STORE_SECRET_KEY=rustfsadmin
```

### 4. Dashboard says token required

Symptom:

```text
OSAI dashboard token required to ask osai
```

Cause:

```text
OSAI_AGENT_TOKEN is set, so /api/ask requires X-OSAI-Token.
```

Local dev fix:

```bash
grep OSAI_AGENT_TOKEN .env.storage
# set it empty:
# OSAI_AGENT_TOKEN=
```

Then restart:

```bash
RUST_LOG=info ./target/release/osai-all
```

Production/API token fix:

```bash
export OSAI_AGENT_TOKEN='your-long-token'

curl -H "X-OSAI-Token: $OSAI_AGENT_TOKEN" \
  http://127.0.0.1:8000/api/health | jq
```

### 5. Cognee recall is slow or failing

Check Cognee and llama first:

```bash
curl http://127.0.0.1:8001/docs
curl http://127.0.0.1:8080/v1/models
docker logs osai-cognee --tail 100
docker logs osai-llama --tail 100
```

Check `.env.cognee`:

```env
COGNEE_API_URL=http://127.0.0.1:8001
COGNEE_API_PREFIX=/api/v1
COGNEE_DATASET=osai-agent-memory
OSAI_LLM_ENDPOINT=http://127.0.0.1:8080/v1
OSAI_LLM_MODEL=osai-llm
```

For Cognee Cloud, use your tenant API URL and API key.

---

## Clean rebuild

Use when old containers, volumes, or binaries are confusing the test.

Stop services:

```bash
docker compose -f docker-compose.storage.yml down
```

Remove generated Rust build output:

```bash
cargo clean
```

Rebuild and run:

```bash
cargo fmt
cargo check
cargo build --release
RUST_LOG=info ./target/release/osai-all
```

Do not delete Docker volumes unless you intentionally want to lose PostgreSQL/RustFS/Cognee local data.

---

## Safe remote exposure

Local-only development:

```bash
./target/release/osai-agent --bind 127.0.0.1:8000 --scan-interval-seconds 30
```

Remote dashboard:

```bash
export OSAI_AGENT_TOKEN='change-me-long-random-token'
./target/release/osai-agent --bind 0.0.0.0:8000 --scan-interval-seconds 30
```

Firewall and reverse proxy decisions should be handled separately. Do not expose the dashboard publicly without auth.

---

## Important files

```text
src/main.rs                         API and dashboard server
src/ask.rs                          /api/ask orchestration
src/ask_plan.rs                     intent detection
src/fact_pack.rs                    focused facts for Qwen
src/bin/osai-all.rs                 full runtime supervisor
src/bin/osai-storage-worker.rs      PostgreSQL/RustFS persistence
src/bin/osai-cognee-ingest.rs       Cognee memory ingestion
docker-compose.storage.yml          local support stack
scripts/ensure-rustfs-bucket.sh     manual RustFS bucket helper
.env.storage.example                OSAI DB/object-store config
.env.cognee.example                 Cognee/Qwen config
models/                             local GGUF model folder
web/app.js                          dashboard Ask OSAI UI
```

---

## References

- GitHub clone documentation: https://docs.github.com/articles/cloning-a-repository
- Git clone documentation: https://git-scm.com/docs/git-clone
- Cargo documentation: https://doc.rust-lang.org/cargo/
- cargo fmt documentation: https://doc.rust-lang.org/cargo/commands/cargo-fmt.html
- Docker Compose services: https://docs.docker.com/reference/compose-file/services/
- Docker Compose startup order: https://docs.docker.com/compose/how-tos/startup-order/
- Qwen3-4B GGUF model: https://huggingface.co/Qwen/Qwen3-4B-GGUF
- Cognee REST API server: https://docs.cognee.ai/guides/deploy-rest-api-server
- Cognee API reference: https://docs.cognee.ai/api-reference/introduction
- RustFS Docker installation: https://docs.rustfs.com/installation/docker/
