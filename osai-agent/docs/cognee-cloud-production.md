# Cognee Cloud Production Setup

> File guide:
> - Purpose: Production notes for using Cognee Cloud with OSAI memory ingestion and recall.
> - Where this fits in OSAI: Guides .env.cognee, osai-cognee-ingest, and Ask OSAI cloud-memory configuration.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Credentials belong in local env files, not committed examples.



This project should use Cognee Cloud as the heavy memory layer when the local machine is low on CPU/RAM.

## Where To Put Cognee Details

Copy the example file:

```bash
cp .env.cognee.example .env.cognee
```

Then edit `.env.cognee`:

```bash
COGNEE_API_URL=https://your-cognee-api-base-url
COGNEE_API_PREFIX=/api/v1
COGNEE_API_KEY=your-api-key
COGNEE_TENANT_ID=your-tenant-id
COGNEE_USER_ID=your-user-id
COGNEE_DATASET=osai-agent-memory
```

Keep these defaults unless Cognee support asks otherwise:

```bash
OSAI_COGNEE_SEND_IDENTITY_HEADERS=false
OSAI_COGNEE_SEND_BEARER_AUTH=false
OSAI_COGNEE_RECALL_WITH_AI_OFF=true
OSAI_COGNEE_RUN_IN_BACKGROUND=true
OSAI_COGNEE_CHUNKS_PER_BATCH=4
```

Cognee Cloud REST API authentication uses:

```text
X-Api-Key: your-api-key
```

The Tenant ID and User ID are kept in config for operator clarity and future audit tagging. The documented Cloud REST path only requires the API key.

## Verify The Cognee Cloud URL

Run these after editing `.env.cognee`:

```bash
set -a
. ./.env.cognee
set +a

curl -i "$COGNEE_API_URL/health" \
  -H "X-Api-Key: $COGNEE_API_KEY"

curl -i "$COGNEE_API_URL$COGNEE_API_PREFIX/recall" \
  -H "X-Api-Key: $COGNEE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"query":"ping","datasets":["osai-agent-memory"],"top_k":1,"only_context":true}'
```

If recall returns `404 Not Found`, try:

```bash
sed -i 's#^COGNEE_API_PREFIX=.*#COGNEE_API_PREFIX=/api#' .env.cognee
```

Then rerun the recall curl test.

## Low-Resource Run Mode

Start only the lightweight local services:

```bash
docker compose -f docker-compose.storage.yml up -d postgres rustfs
```

Skip local Cognee:

```bash
# Do not start this on low-resource machines:
# docker compose -f docker-compose.storage.yml up -d cognee
```

Start local llama only when you want optional AI refinement and the model is present:

```bash
docker compose -f docker-compose.storage.yml up -d llama
```

The dashboard works without local llama. Ask OSAI will use:

```text
Rust live scan + Cognee Cloud recall
```

When the AI toggle is off, Ask OSAI does not call local Qwen.

## Memory Write Path

Run the storage worker:

```bash
./target/release/osai-storage-worker
```

Run the Cognee Cloud ingestion bridge:

```bash
./target/release/osai-cognee-ingest
```

The ingestion bridge reads pending memory rows from PostgreSQL and sends compact Markdown memories to:

```text
POST <COGNEE_API_URL>/api/v1/remember
```

It sends the API key as `X-Api-Key` and uploads memory as multipart Markdown files.

## Ask OSAI Read Path

Ask OSAI now recalls Cognee Cloud even when local AI is off:

```text
POST <COGNEE_API_URL>/api/v1/recall
```

The Rust answer can include a trimmed `Recalled Cognee Memory` section. This keeps the machine fast while still using long-term cloud memory.

## What To Remember

Do remember:

- new warning or critical findings
- incidents and resolved fixes
- command outputs from approved actions
- service discovery changes
- GitLab/Kubernetes/Postgres problems
- previous fix that worked

Do not remember every full scan. PostgreSQL keeps exact scan facts locally; Cognee should keep meaning, relationships, incidents, and reusable lessons.
