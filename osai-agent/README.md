# OSAI Agent

> File guide:
> - Purpose: Main operator guide for understanding, configuring, running, and extending OSAI Agent.
> - Where this fits in OSAI: This is the first document humans and AI assistants should read before touching Docker, Rust binaries, or model deployment.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Keep commands aligned with the two supported llama modes: host-mounted GGUF and baked model image.



OSAI Agent is a **Rust-first local Linux and DevOps intelligence agent**. It scans the machine, stores exact facts, archives raw evidence, prepares AI memory, and can ask a local llama.cpp/Qwen model using recalled Cognee context.

The current project is intentionally split into Rust binaries plus Docker services:

```text
Rust binaries:
  osai-agent             = scanner + dashboard + API + guarded actions
  osai-storage-worker    = PostgreSQL + RustFS persistence worker
  osai-cognee-ingest     = pushes pending memory rows into Cognee REST
  osai-ask               = recalls Cognee memory + asks llama.cpp/Qwen

Docker Compose services:
  postgres               = operational DB + Cognee DB + pgvector
  rustfs                 = S3-compatible raw evidence store
  rustfs-init            = creates the osai-agent bucket automatically
  llama                  = llama.cpp server running Qwen3-4B-Q4_K_M.gguf
  cognee                 = Cognee REST API memory/retrieval server
```

## Mental model

```text
Rust scans and controls.
PostgreSQL stores exact facts and metadata.
RustFS stores raw JSON/log/report objects.
Cognee stores searchable AI memory.
pgvector finds semantically similar memory.
Kuzu stores relationships/graph memory.
llama.cpp runs Qwen locally.
Rust sends Qwen only the useful retrieved context.
```

Qwen does **not** directly read PostgreSQL, RustFS, pgvector, or Kuzu. Rust fetches facts and memory first, builds a safe prompt, sends that prompt to llama.cpp/Qwen, and then can save the answer back.

## Data format strategy

OSAI now keeps two versions of each scan:

```text
Raw JSON snapshot      = exact machine evidence, stored in RustFS and PostgreSQL JSONB
Markdown memory file   = descriptive human-readable summary, stored in RustFS, PostgreSQL, and Cognee
```

The Markdown memory file is the preferred input for Cognee because it has headings, bullet points, findings, recommendations, evidence links, and tags. Markdown is plain text but structured enough for humans and LLM/RAG systems to read cleanly. Raw JSON is still preserved as evidence, but it is not the main memory format sent to Cognee.


## What the project can do now

- Serve a browser dashboard from the Rust binary.
- Expose API endpoints for health, snapshot, history, knowledge, plugins, reasoning, and guarded actions.
- Scan Linux host state: OS, CPU, memory, disk, processes, ports, Kubernetes signals, and GitLab signals.
- Apply rule findings for high memory, high CPU, disk pressure, sensitive ports, Kubernetes detection, GitLab detection, and known GitLab auto-start memory pattern.
- Keep local JSONL scan history in `data/scan_history.jsonl`.
- Persist structured scan metadata to PostgreSQL.
- Persist full raw scan JSON to RustFS.
- Convert each scan into descriptive Markdown memory that is readable by humans and easier for Cognee/Qwen to understand.
- Persist Markdown memory to RustFS under `memory/scans/...`.
- Store the same Markdown memory text and metadata in PostgreSQL.
- Push useful Markdown memory rows to Cognee over REST as uploaded `.md` files. Raw scans are stored every cycle, while Cognee receives first scan, changed state, important state refreshes, and periodic summaries.
- Ask local Qwen via llama.cpp using PostgreSQL facts plus recalled Cognee memory. AI is a refinement layer; Rust remains the source of truth.
- Ask OSAI from the browser dashboard through `/api/ask`, without using the llama.cpp UI directly.

## Project structure

```text
osai-agent/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── docker-compose.storage.yml
├── docker-compose.model-image.yml
├── .env.storage.example
├── .env.cognee.example
├── config/
│   └── osai-agent.toml
├── docker/
│   ├── cognee/
│   │   └── Dockerfile
│   ├── llama/
│   │   └── Dockerfile
│   └── llama-model/
│       ├── Dockerfile
│       └── Dockerfile.dockerignore
├── docs/
│   ├── llama-qwen-cognee-rust-architecture.md
│   ├── qwen3-gguf-loading-footprint.md
│   ├── phase3-storage-cognee.md
│   ├── rust-storage-worker.md
│   ├── rust-to-cognee-to-qwen-data-flow.md
│   └── understanding-model-metadata-like-a-pro.md
├── knowledge/
│   └── *.md
├── models/
│   └── Qwen3-4B-Q4_K_M.gguf  # local file, ignored by git
├── packaging/
│   ├── rpm/
│   └── systemd/
├── scripts/
│   ├── ask-local-qwen.sh
│   ├── build-llama-model-image.sh
│   ├── build-rpm.sh
│   ├── run-cognee-ingest.sh
│   ├── run-memory-worker.sh
│   └── setup-storage-venv.sh  # deprecated; use Docker Compose for Cognee
├── src/
│   ├── main.rs
│   ├── actions.rs
│   ├── ask.rs
│   ├── history.rs
│   ├── knowledge.rs
│   ├── reasoning.rs
│   ├── rules.rs
│   ├── bin/
│   │   ├── osai-storage-worker.rs
│   │   ├── osai-cognee-ingest.rs
│   │   └── osai-ask.rs
│   ├── collector/
│   └── plugins/
├── storage/
│   ├── postgres-init/
│   └── migrations/
└── web/
    ├── index.html
    ├── app.css
    └── app.js
```

## Service dependency flow

Start in this order:

```text
1. PostgreSQL + RustFS + llama.cpp/Qwen + Cognee container stack
2. osai-agent Rust server
3. osai-storage-worker
4. osai-cognee-ingest
5. Browser Ask OSAI or osai-ask CLI
```

Dependency map:

```text
osai-agent
  depends on: no external service for basic dashboard/scanning

osai-storage-worker
  depends on: osai-agent API, PostgreSQL, RustFS bucket

osai-cognee-ingest
  depends on: PostgreSQL, Cognee REST API

osai-ask
  depends on: PostgreSQL, Cognee REST API, llama.cpp/Qwen

cognee container
  depends on: PostgreSQL, llama Docker service for ingestion/extraction quality
```

## 1. Prepare environment files

```bash
cp .env.storage.example .env.storage
cp .env.cognee.example .env.cognee
```

Important config files:

```text
.env.storage = Rust worker config for osai-agent API, PostgreSQL, RustFS, dataset names
.env.cognee  = Cognee container config plus local llama.cpp/Qwen config
```

## 2. Put the Qwen GGUF model in place

Create the local model folder and place your model here:

```text
models/Qwen3-4B-Q4_K_M.gguf
```

The `models/` folder is mounted into the llama.cpp container. The `.gguf` file is intentionally ignored by git because it is large.

Expected path:

```bash
ls -lh models/Qwen3-4B-Q4_K_M.gguf
```

If the filename is different, either rename it or update the `llama` service command in both compose files and set `MODEL_FILE` when building the baked image.

You can also pass another GGUF filename without editing compose:

```bash
OSAI_GGUF_MODEL_FILE=Qwen3-4B-IQ3_M.gguf docker compose -f docker-compose.storage.yml up -d --build

MODEL_FILE=Qwen3-4B-IQ3_M.gguf ./scripts/build-llama-model-image.sh
OSAI_GGUF_MODEL_FILE=Qwen3-4B-IQ3_M.gguf docker compose -f docker-compose.model-image.yml up -d --build
```

### Fast Qwen loading rule

For this project, there are now two supported Qwen deployment modes.

Mode A is best for development and local testing:

```text
runtime image = llama.cpp server only
model file    = local ./models/Qwen3-4B-Q4_K_M.gguf mounted read-only
startup       = no runtime download, explicit --mmap
context       = keep -c modest unless you truly need long context
```

Mode B is best when you want to push one image to another server:

```text
runtime image = llama.cpp server plus Qwen GGUF inside image
model file    = /models/Qwen3-4B-Q4_K_M.gguf inside the container image
startup       = no runtime download and no host model mount, explicit --mmap
tradeoff      = image is larger by the model size
```

Do not bake the GGUF into normal source zips or Git commits. For one server, Mode A is usually simpler. For repeatable deploys across servers, Mode B avoids downloading or copying the model at container startup.

See `docs/qwen3-gguf-loading-footprint.md` for the full deployment note.

## 3. Start Docker Compose services

Choose one llama mode.

### Option A: host-mounted model

```bash
docker compose -f docker-compose.storage.yml up -d --build
```

This keeps the image small and uses `./models:/models:ro`.

### Option B: Docker image contains the model

Build and run the baked model image:

```bash
./scripts/build-llama-model-image.sh
docker compose -f docker-compose.model-image.yml up -d --build
```

This copies `models/Qwen3-4B-Q4_K_M.gguf` into `osai-llama-qwen-with-model:local`. It still uses llama.cpp `--mmap`, but it does not need a host model mount at runtime.

This starts:

```text
osai-postgres    PostgreSQL + pgvector
osai-rustfs      RustFS object store
osai-rustfs-init automatic bucket bootstrap for s3://osai-agent
osai-llama       llama.cpp server running Qwen3 on port 8080
osai-cognee      Cognee REST API on port 8001
```

Check status:

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

Open RustFS console:

```text
http://127.0.0.1:9001
username: rustfsadmin
password: rustfsadmin
```

Open Cognee docs:

```text
http://127.0.0.1:8001/docs
```

Test llama.cpp/Qwen:

```bash
curl http://127.0.0.1:8080/v1/models
```

Why the endpoints differ:

```text
Rust host binaries call llama.cpp at:
  http://127.0.0.1:8080/v1

Cognee container calls llama.cpp through Docker service DNS:
  http://llama:8080/v1
```

## 4. Build Rust binaries

```bash
cargo check
cargo build --release
```

Release binaries:

```text
target/release/osai-agent
target/release/osai-storage-worker
target/release/osai-cognee-ingest
target/release/osai-ask
```

## 5. Start the Rust agent

```bash
./target/release/osai-agent --bind 127.0.0.1:8000 --scan-interval-seconds 30
```

Open dashboard:

```text
http://127.0.0.1:8000
```

Basic API checks:

```bash
curl http://127.0.0.1:8000/api/health
curl http://127.0.0.1:8000/api/snapshot | jq
curl http://127.0.0.1:8000/api/history | jq
```

## 6. Persist scan data into PostgreSQL and RustFS

Run once:

```bash
./target/release/osai-storage-worker --once
```

Or use the wrapper:

```bash
./scripts/run-memory-worker.sh --once
```

Run continuously:

```bash
./target/release/osai-storage-worker
```

This writes:

```text
PostgreSQL:
  osai_hosts
  osai_scan_history
  osai_findings
  osai_memory_events
  osai_cognee_outbox

RustFS:
  s3://osai-agent/snapshots/<hostname>/<generated-at>/<scan-id>.json
  s3://osai-agent/memory/scans/<hostname>/<generated-at>/<scan-id>.md
```

The `.json` file is the raw evidence. The `.md` file is the descriptive memory document that humans, Cognee, pgvector/Kuzu, and Qwen can understand more easily.

To rebuild Markdown memory for scans that were already saved before this feature existed, run:

```bash
./target/release/osai-storage-worker --once --rebuild-memory
```

## 7. Verify PostgreSQL data

```bash
docker exec -it osai-postgres psql -U postgres -d osai_agent -c '\dt'
```

Latest scans:

```bash
docker exec -it osai-postgres psql -U postgres -d osai_agent -c "
select id, generated_at, hostname, highest_severity, finding_count, object_store_key
from osai_scan_history
order by generated_at desc
limit 5;
"
```

Memory/outbox:

```bash
docker exec -it osai-postgres psql -U postgres -d osai_agent -c "
select id, scan_id, status, attempt_count, last_error, ingested_at
from osai_cognee_outbox
order by id desc
limit 10;
"
```

pgvector extension in Cognee DB:

```bash
docker exec -it osai-postgres psql -U postgres -d cognee_db -c "
select extname, extversion from pg_extension where extname = 'vector';
"
```

## 8. Verify RustFS data from CLI

Using AWS CLI:

```bash
export AWS_ACCESS_KEY_ID=rustfsadmin
export AWS_SECRET_ACCESS_KEY=rustfsadmin
export AWS_DEFAULT_REGION=us-east-1
export AWS_EC2_METADATA_DISABLED=true

aws --endpoint-url http://127.0.0.1:9000 s3 ls
aws --endpoint-url http://127.0.0.1:9000 s3 ls s3://osai-agent/ --recursive
```

Read one object:

```bash
aws --endpoint-url http://127.0.0.1:9000 s3 cp \
  s3://osai-agent/<object_store_key_from_postgres> - | jq .
```

Read generated Markdown memory:

```bash
aws --endpoint-url http://127.0.0.1:9000 s3 ls s3://osai-agent/memory/scans/ --recursive
aws --endpoint-url http://127.0.0.1:9000 s3 cp \
  s3://osai-agent/<memory_markdown_object_key_from_metadata> -
```

## 9. Ingest pending memory into Cognee

Run once:

```bash
./target/release/osai-cognee-ingest --once
```

Or use the wrapper:

```bash
./scripts/run-cognee-ingest.sh --once
```

Expected outbox status:

```text
ingested
```

Check:

```bash
docker exec -it osai-postgres psql -U postgres -d osai_agent -c "
select id, scan_id, status, attempt_count, last_error, ingested_at
from osai_cognee_outbox
order by id desc
limit 10;
"
```

If it fails, check:

```bash
docker logs osai-cognee --tail 100
```

### Cognee remember endpoint detail

Cognee 1.2.x expects `/api/v1/remember` field `data` to be uploaded as a multipart file, not a plain text form field. The Rust `osai-cognee-ingest` binary sends each descriptive Markdown memory row as an in-memory `.md` upload. This matches the manual test below:

```bash
printf "# OSAI Debug\n\nhello from osai debug" > /tmp/osai-debug.md
curl -i -X POST http://127.0.0.1:8001/api/v1/remember \
  -F "data=@/tmp/osai-debug.md;type=text/markdown" \
  -F "datasetName=osai_debug" \
  -F "run_in_background=false"
```

If you see `Expected UploadFile, received: <class 'str'>`, rebuild the Rust binaries because an older `osai-cognee-ingest` binary is still sending `data` as text.

`remember()` can take minutes on CPU because Cognee builds graph/vector memory and calls the local LLM. The worker timeout is controlled by:

```env
OSAI_COGNEE_HTTP_TIMEOUT_SECONDS=900
OSAI_COGNEE_RUN_IN_BACKGROUND=false
OSAI_COGNEE_CHUNKS_PER_BATCH=10
```


## 10. Ask OSAI from browser, CLI, or API

### Browser

Open the dashboard:

```text
http://127.0.0.1:8000
```

Use the **Ask OSAI** panel. The browser calls:

```text
POST /api/ask
```

Request path:

```text
Browser
  -> osai-agent /api/ask
  -> PostgreSQL latest scan facts
  -> Cognee /api/v1/recall
  -> local Markdown guidance from knowledge/
  -> llama.cpp /v1/chat/completions
  -> Qwen answer returned to dashboard
```

Editable answer guidance lives here:

```text
knowledge/09_inference_reasoning_guidance.md
```

Use that file to add real issue patterns, response rules, known errors, and service-specific guidance.

### CLI

```bash
./target/release/osai-ask "give me update about this server"
```

Or:

```bash
./scripts/ask-local-qwen.sh "give me update about this server"
```

Request path:

```text
osai-ask
  -> PostgreSQL latest scan facts
  -> Cognee /api/v1/recall
  -> llama.cpp /v1/chat/completions
  -> Qwen answer printed in CLI
```

### API

```bash
curl -X POST http://127.0.0.1:8000/api/ask \
  -H 'Content-Type: application/json' \
  -d '{"question":"give me update about this server"}' | jq
```

Fast deterministic fallback without Qwen:

```bash
curl -X POST http://127.0.0.1:8000/api/reason \
  -H 'Content-Type: application/json' \
  -d '{"question":"why is memory high?"}' | jq
```

## Browser, CLI, and API access

Browser dashboard:

```text
http://127.0.0.1:8000
```

RustFS console:

```text
http://127.0.0.1:9001
```

Cognee REST docs:

```text
http://127.0.0.1:8001/docs
```

CLI examples:

```bash
./target/release/osai-storage-worker --once
./target/release/osai-cognee-ingest --once
./target/release/osai-ask "what changed on this machine?"
```

API examples:

```bash
curl http://127.0.0.1:8000/api/health
curl http://127.0.0.1:8000/api/snapshot | jq
curl http://127.0.0.1:8000/api/history | jq
curl -X POST http://127.0.0.1:8000/api/scan | jq
curl -X POST http://127.0.0.1:8000/api/ask \
  -H 'Content-Type: application/json' \
  -d '{"question":"what is the current server status?"}' | jq
```

## Expose dashboard outside localhost safely

When binding to `0.0.0.0`, set an API token:

```bash
export OSAI_AGENT_TOKEN='change-me-long-random-token'
./target/release/osai-agent --bind 0.0.0.0:8000
```

API call with token:

```bash
curl -H "X-OSAI-Token: $OSAI_AGENT_TOKEN" http://127.0.0.1:8000/api/snapshot | jq
```

## Guarded command executor

The executor validates commands against an allowlist, blocks destructive programs, rejects shell metacharacters, audits actions, and requires approval for repair actions.

Propose a read-only check:

```bash
curl -X POST http://127.0.0.1:8000/api/actions/propose \
  -H 'Content-Type: application/json' \
  -d '{
    "reason":"check GitLab status",
    "command":"gitlab-ctl",
    "args":["status"],
    "kind":"read_only"
  }' | jq
```

Run the returned action id:

```bash
curl -X POST http://127.0.0.1:8000/api/actions/<action-id>/run | jq
```

## Important design rules

Do not use Cognee as the only database for system state.

```text
PostgreSQL = operational source of truth
RustFS     = raw evidence vault
Cognee     = AI memory/retrieval layer
Qwen       = reasoning model, not storage
Rust       = controller and guardrail layer
```

Do not send raw RustFS logs directly into Qwen by default. Let Rust curate facts, let Cognee recall relevant memory, and send Qwen a clean prompt.

## Cleaning heavy folders

These folders are generated and should not be committed:

```text
target/
.venv-storage/
data/
models/*.gguf
```

Clean Rust build cache:

```bash
cargo clean
```

Remove old Python venv if it exists:

```bash
rm -rf .venv-storage scripts/.venv-storage
```

## RPM/systemd packaging

Systemd and RPM skeletons remain under:

```text
packaging/systemd/
packaging/rpm/
```

Build RPM on a RHEL-like host:

```bash
sudo dnf install -y rust cargo rpm-build rsync systemd-rpm-macros
./scripts/build-rpm.sh
```

## References

- Cognee REST API server: https://docs.cognee.ai/guides/deploy-rest-api-server
- Cognee installation and extras: https://docs.cognee.ai/getting-started/installation
- Cognee LLM providers: https://docs.cognee.ai/setup-configuration/llm-providers
- RustFS Docker installation: https://docs.rustfs.com/installation/docker/
- Docker Compose services: https://docs.docker.com/reference/compose-file/services/
- Full OSAI data flow: `docs/rust-to-cognee-to-qwen-data-flow.md`
- Qwen GGUF loading and footprint guide: `docs/qwen3-gguf-loading-footprint.md`
