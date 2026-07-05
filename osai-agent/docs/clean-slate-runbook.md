# Clean Slate Runbook

> File guide:
> - Purpose: Runbook for resetting the local OSAI stack and starting from a clean state.
> - Where this fits in OSAI: Operator recovery guide when Docker volumes, databases, or memory state need to be rebuilt.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Destructive cleanup commands should remain explicit and clearly scoped.



Use this when Docker containers or volumes were cleared and OSAI is starting from zero.

## 1. Stop Old Containers

```bash
docker compose -f docker-compose.storage.yml down --remove-orphans
docker rm -f osai-postgres osai-rustfs osai-rustfs-init osai-llama osai-cognee 2>/dev/null || true
```

Only remove volumes when you intentionally want to delete local OSAI state:

```bash
docker volume ls -q | grep 'osai.*\(postgres\|rustfs\|cognee\)' | xargs -r docker volume rm
```

## 2. Create Env Files

Both files are needed.

```bash
cp .env.storage.example .env.storage
cp .env.cognee.example .env.cognee
```

Edit `.env.cognee` and add Cognee Cloud values:

```bash
COGNEE_API_URL=https://your-cognee-api-base-url
COGNEE_API_PREFIX=/api/v1
COGNEE_API_KEY=your-api-key
COGNEE_TENANT_ID=your-tenant-id
COGNEE_USER_ID=your-user-id
COGNEE_DATASET=osai-agent-memory
```

## 3. Start Local Storage

```bash
docker compose -f docker-compose.storage.yml up -d postgres rustfs
docker compose -f docker-compose.storage.yml up rustfs-init
```

The `rustfs-init` service now uses `mc mb --ignore-existing` and creates the required bucket automatically:

```text
osai-agent
```

If the worker logs `NoSuchBucket`, run the init service again:

```bash
docker compose -f docker-compose.storage.yml up rustfs-init
```

Verify the bucket:

```bash
docker compose -f docker-compose.storage.yml up rustfs-init
```

Verify services:

```bash
docker compose -f docker-compose.storage.yml ps
curl http://127.0.0.1:9000
```

## 4. Build Rust

```bash
cargo build --release
```

## 5. Start OSAI Runtime

Terminal 1:

```bash
./target/release/osai-agent \
  --bind 127.0.0.1:8000 \
  --scan-interval-seconds 30
```

Terminal 2:

```bash
./target/release/osai-storage-worker
```

Terminal 3:

```bash
./target/release/osai-cognee-ingest
```

## 6. Optional Local Inference

Only start llama/Qwen when the model exists and the machine can handle it:

```bash
ls -lh models/Qwen3-4B-Q4_K_M.gguf
docker compose -f docker-compose.storage.yml up -d llama
curl http://127.0.0.1:8080/health
```

Local inference is optional. The default low-resource path is:

```text
Rust live scan + Cognee Cloud recall
```

When the machine can run local Qwen, turn on the AI button in the UI. Rust still owns the scan facts, commands, and guardrails. Qwen only refines the current Rust facts plus Cognee recall into natural operator language.

## Known Harmless Log

This log is harmless:

```text
static asset not found: favicon.ico
```

It only means the browser requested `/favicon.ico` and the dashboard does not include one yet.

## Cognee 404

If Ask OSAI logs:

```text
Cognee recall endpoint returned 404 Not Found
```

verify the Cloud URL and prefix:

```bash
set -a
. ./.env.cognee
set +a

curl -i "$COGNEE_API_URL/health" -H "X-Api-Key: $COGNEE_API_KEY"
curl -i "$COGNEE_API_URL$COGNEE_API_PREFIX/recall" \
  -H "X-Api-Key: $COGNEE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"query":"ping","datasets":["osai-agent-memory"],"top_k":1,"only_context":true}'
```

If `/api/v1/recall` returns 404 but `/api/recall` works, change:

```bash
COGNEE_API_PREFIX=/api
```
