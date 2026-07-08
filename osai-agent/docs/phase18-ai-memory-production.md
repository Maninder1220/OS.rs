# Phase 18: AI Reasoning And Cleaner Memory

> File guide:
> - Purpose: Phase18 production summary for AI memory, recall, and operational deployment.
> - Where this fits in OSAI: High-level release note for the current memory/reasoning milestone.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Keep this synchronized with actual Docker Compose and worker behavior.



This phase keeps OSAI predictable while making the final answer more natural.

## Runtime Flow

```text
Rust scanner -> PostgreSQL exact facts -> RustFS raw evidence
             -> Cognee Cloud memory -> optional llama.cpp/Qwen refinement
```

Rust remains the source of truth. The AI layer is only a reasoning and wording layer. It receives the current Rust insight, latest stored scan facts, Cognee recall, and local guidance. It must not invent metrics or claim a repair action happened.

## Cognee Memory Policy

The scanner may run every 30 seconds, but Cognee should not receive the same healthy scan forever.

Default behavior:

- Store raw scan JSON in PostgreSQL and RustFS every cycle.
- Send the first scan for a host to Cognee.
- Send to Cognee immediately when the server state signature changes.
- Send periodic summaries every `OSAI_COGNEE_MEMORY_MIN_INTERVAL_SECONDS`.
- Keep warning and critical states refreshed on the same interval.

Default settings:

```bash
OSAI_COGNEE_MEMORY_MIN_INTERVAL_SECONDS=900
OSAI_COGNEE_MEMORY_ALWAYS_ON_FINDINGS=true
```

This keeps long-term memory useful for repeated incidents without filling Cognee with duplicate "nothing changed" rows.

## Inference Layer Settings

Use these when local llama.cpp/Qwen is available:

```bash
OSAI_LLM_ENDPOINT=http://127.0.0.1:8080/v1
OSAI_LLM_MODEL=osai-llm
OSAI_LLM_TIMEOUT_SECONDS=180
OSAI_LLM_MAX_TOKENS=420
```

The UI AI button controls whether `/api/ask` calls local Qwen. If AI is off or not ready, OSAI still answers from Rust insights plus Cognee recall.

## RustFS Bucket Bootstrap

`rustfs-init` now uses the MinIO `mc` client:

```bash
docker compose -f docker-compose.storage.yml up rustfs-init
```

It runs:

```bash
mc mb --ignore-existing rustfs/osai-agent
```

So a clean slate run should create the `osai-agent` bucket without manual `docker run` commands.
