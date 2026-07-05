# Inference And Reasoning Layer Health

> File guide:
> - Purpose: Explains health checks and failure modes for the local inference layer.
> - Where this fits in OSAI: Supports diagnosing llama.cpp/Qwen availability before blaming Ask OSAI.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Use direct /health and /v1/models checks before debugging Cognee or prompt logic.



Ask OSAI now treats llama.cpp/Qwen as a reasoning layer, not as the source of truth.

Rust owns the facts:

```text
scanner snapshot -> deterministic insights -> optional Qwen refinement
```

Qwen refines the explanation only when the inference layer is ready.

## Health Detection

Before calling:

```text
POST http://127.0.0.1:8080/v1/chat/completions
```

Rust checks:

```text
GET http://127.0.0.1:8080/health
```

If health is ready, Rust sends the structured server data to Qwen.

If health is not ready, Rust skips Qwen and returns deterministic Rust insight cards.

## What 503 Means

`503 Service Unavailable` from llama.cpp usually means the inference server is not ready.

Common causes:

- model is still loading
- model path is wrong
- model file is missing
- server crashed or restarted
- CPU/RAM is overloaded
- context/model size is too large for the machine

For Qwen3-4B GGUF, first startup can still take time because the operating system must read model pages from disk. Later restarts are often faster when the OS page cache is warm.

## Fast Loading Checklist

- Keep `models/Qwen3-4B-Q4_K_M.gguf` on local disk.
- For development mode, keep the model mounted read-only into the llama container.
- For model-image mode, build `osai-llama-qwen-with-model:local` so the GGUF exists inside the image.
- Use `--mmap`; the compose file makes this explicit.
- Avoid downloading the model at container startup.
- Keep context modest for normal troubleshooting. A larger context increases KV-cache memory.
- Use `--mlock` only on hosts with enough RAM and memlock permission.

## Manual Checks

Run these on the server:

```bash
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:8080/v1/models
docker compose -f docker-compose.storage.yml ps
docker compose -f docker-compose.model-image.yml ps
docker logs osai-llama --tail 100
ls -lh models/Qwen3-4B-Q4_K_M.gguf
free -m
df -h
```

## Response Behavior

Ask OSAI now returns:

```text
inference_status.ready
inference_status.status
inference_status.detail
inference_status.recommended_checks
query_insights[].manual_checks
```

The browser shows this as:

- reasoning ready / not ready chip
- health URL
- error detail
- manual checks
- Rust insight cards
- Qwen-refined answer when available

## Design Rule

If Qwen is down, OSAI still works.

If Qwen is healthy, OSAI becomes more explanatory.

Rust still owns the facts.
