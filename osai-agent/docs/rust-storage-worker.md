# Rust Storage Worker

> File guide:
> - Purpose: Explains the Rust storage worker and how it persists scans and memory.
> - Where this fits in OSAI: Companion document for src/bin/osai-storage-worker.rs.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Worker docs should mention both raw evidence and Markdown memory output.



The old Python storage worker has been replaced by `src/bin/osai-storage-worker.rs`.

## What It Does

```text
Rust OSAI Agent API
  -> Rust storage worker
      -> PostgreSQL structured rows
      -> RustFS/S3 raw JSON snapshots
      -> PostgreSQL memory events
      -> PostgreSQL Cognee outbox rows
```

The worker does not run Python and does not create `.venv-storage/`.

## Why Cognee Is Still an Outbox

Cognee runs as a separate Docker Compose service. Instead of pulling the Cognee dependency tree into this Rust repo, the Rust worker stores compact memory text in `osai_memory_events` and creates pending rows in `osai_cognee_outbox`. The Rust `osai-cognee-ingest` binary then calls Cognee's REST API.

That gives us a clean handoff point:

- PostgreSQL remains the operational source of truth.
- RustFS remains the raw evidence store.
- `osai-cognee-ingest` consumes the outbox through Cognee's HTTP API.

## Build

```bash
cargo build --release --bin osai-agent
cargo build --release --bin osai-storage-worker
```

## Run

Start PostgreSQL, RustFS, and Cognee:

```bash
docker compose -f docker-compose.storage.yml up -d --build
```

Start the Rust agent:

```bash
cargo run --bin osai-agent -- --bind 127.0.0.1:8000 --scan-interval-seconds 30
```

Run one storage sync:

```bash
cargo run --bin osai-storage-worker -- --once
```

Run continuously:

```bash
cargo run --bin osai-storage-worker
```

## Environment

Copy examples first:

```bash
cp .env.storage.example .env.storage
cp .env.cognee.example .env.cognee
```

Important values:

```text
OSAI_AGENT_URL=http://127.0.0.1:8000
OSAI_POSTGRES_DSN=postgresql://osai:osai_password@127.0.0.1:5432/osai_agent
OBJECT_STORE_ENDPOINT=127.0.0.1:9000
OBJECT_STORE_ACCESS_KEY=rustfsadmin
OBJECT_STORE_SECRET_KEY=rustfsadmin
OBJECT_STORE_BUCKET=osai-agent
OBJECT_STORE_SECURE=false
OBJECT_STORE_REGION=us-east-1
COGNEE_DATASET=osai-agent-memory
```

## Verify

PostgreSQL:

```bash
psql 'postgresql://osai:osai_password@127.0.0.1:5432/osai_agent' \
  -c 'select id, hostname, highest_severity, finding_count from osai_scan_history order by generated_at desc limit 5;'
```

Cognee outbox:

```bash
psql 'postgresql://osai:osai_password@127.0.0.1:5432/osai_agent' \
  -c 'select id, scan_id, status, content_hash from osai_cognee_outbox order by id desc limit 5;'
```

RustFS:

```text
http://127.0.0.1:9001
```

Look inside the `osai-agent` bucket under `snapshots/`.


## Ingest to Cognee

After `osai-storage-worker` creates pending outbox rows, run:

```bash
cargo run --bin osai-cognee-ingest -- --once
```

Then ask with local Qwen/llama.cpp:

```bash
cargo run --bin osai-ask -- "give me update about this server"
```
