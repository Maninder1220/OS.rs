# Phase 3: PostgreSQL + RustFS + Cognee Outbox

> File guide:
> - Purpose: Historical design note for the storage and Cognee integration phase.
> - Where this fits in OSAI: Useful context for why PostgreSQL, RustFS, and Cognee outbox exist.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Treat as background; current phase18 docs are more authoritative for deployment.



## Decision

Use three layers, each with a different responsibility:

1. **PostgreSQL** is the operational source of truth.
   - scan summaries
   - findings
   - host inventory
   - Cognee ingestion outbox
   - searchable JSONB snapshot copy

2. **RustFS** is the raw evidence/object store.
   - full scan snapshots
   - future log bundles
   - future command output bundles
   - future compressed incident packages

3. **Cognee** is the AI memory layer.
   - compact incident memories
   - recurring patterns
   - runbook knowledge
   - cross-session recall
   - graph/vector retrieval

Do not store the exact same responsibility in all three places. PostgreSQL answers operational questions. RustFS preserves raw evidence. Cognee gives reasoning context to the agent.

## Runtime flow

```text
Rust OSAI Agent
  ├─ scans OS/Kubernetes/GitLab
  ├─ keeps local JSONL history for fallback
  └─ exposes local API
        │
        ▼
Rust OSAI Storage Worker
  ├─ pulls /api/history and /api/history/{id}
  ├─ writes structured rows to PostgreSQL
  ├─ writes full snapshot JSON to RustFS/S3
  ├─ creates a durable PostgreSQL outbox row
  └─ writes compact memory events for later Cognee ingestion
        │
        ▼
Cognee
  ├─ relational store: cognee_db PostgreSQL
  ├─ vector store: pgvector initially
  └─ graph store: Kuzu initially, Neo4j later for multi-agent production
```

## Why a separate worker first?

The Rust binary should stay small and safe. It has OS visibility and guarded command execution, so we should not load it with heavy AI/runtime dependencies yet.

The storage worker is now Rust too. It owns PostgreSQL, RustFS/S3 writes, memory event creation, and Cognee outbox creation. Direct Cognee ingestion is handled by `src/bin/osai-cognee-ingest.rs`, which calls the Cognee REST API running from Docker Compose. This keeps the Rust scanner small while still letting Cognee own graph/vector memory creation.

## First production target

Use this on one RHEL/CentOS host first:

- OSAI Rust agent on `127.0.0.1:8000`
- PostgreSQL container on `127.0.0.1:5432`
- RustFS container on `127.0.0.1:9000`
- Rust storage worker running as a normal unprivileged process
- Cognee container on `127.0.0.1:8001` using `cognee_db` for its own metadata and `osai_agent` only for OSAI operational tables

## Later production target

When the agent becomes multi-server:

- one OSAI agent per server
- central PostgreSQL
- central RustFS/S3
- Cognee with Neo4j or another production graph backend
- Qdrant/PGVector/LanceDB depending on scale
- one ingestion worker per queue partition
