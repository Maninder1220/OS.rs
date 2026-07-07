# Cognee Memory Lifecycle Upgrade

> File guide:
> - Purpose: Explains the final memory lifecycle additions: remember, recall, improve-feedback, forget, status, redaction, AskPlan, and FactPack.
> - Where this fits in OSAI: Read this with `src/cognee_lifecycle.rs`, `src/ask_plan.rs`, `src/fact_pack.rs`, `src/ask.rs`, and the dashboard memory panel.
> - Topics to know: Cognee memory APIs, long-term agent memory, graph/vector recall, Rust orchestration, and safe operational redaction.
> - Operational note: Rust facts remain source of truth; Cognee stores memory; Qwen only explains/refines supplied context.

## What Changed

OSAI now uses Cognee as a lifecycle system, not only a remember/recall store.

```text
Remember:
  osai-cognee-ingest sends redacted Markdown memory into Cognee.
  Dashboard feedback also remembers answer usefulness.

Recall:
  AskPlan decides whether the question needs memory.
  Simple live CPU/RAM/disk questions can skip Cognee.
  Service, finding, GitLab, Kubernetes, repeated issue, or history questions use focused recall.

Improve:
  Feedback buttons send answer usefulness back to Cognee.
  OSAI then attempts best-effort Cognee improve.

Forget:
  Dashboard exposes a confirmed forget workflow for stale/noisy/secret-risk memory cleanup.

Status:
  Dashboard shows Cognee lifecycle health and configured dataset.
```

## AskPlan + FactPack Flow

```text
User question
  -> Rust AskPlan
  -> focused FactPack
  -> optional Cognee recall
  -> optional Qwen refinement
  -> feedback/improve lifecycle
```

This prevents low-resource machines from sending unrelated full-server context to Qwen.

## Safety

Before memory is sent to Cognee, OSAI redacts secret-like lines containing:

```text
password=
token=
api_key
Authorization:
Bearer
private key
secret_key
access_key
```

Raw local evidence can still remain in PostgreSQL/RustFS for audit. Cognee memory is the long-term AI layer and should be safer by default.

## Demo Story

1. Start OSAI with `osai-all`.
2. OSAI runs RustFS bucket init before storage writes.
3. Storage worker remembers scan memory into Cognee.
4. User asks: `what is the update on service`.
5. Rust detects `Services`, builds a service FactPack, and recalls relevant memory.
6. Qwen explains only the supplied service facts and memory.
7. User marks answer Helpful or Resolved.
8. OSAI remembers feedback and attempts improve.
9. Operator can forget stale/noisy memory if needed.
