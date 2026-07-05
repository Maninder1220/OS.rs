# Full Server Askables

> File guide:
> - Purpose: Catalogs server questions OSAI can answer from collected facts and memory.
> - Where this fits in OSAI: Helps tune reasoning, intent routing, and Ask OSAI prompts.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Askable examples should map to real scanned fields whenever possible.



This phase expands Ask OSAI from a CPU/memory demo into a full deterministic server overview.

## Default Question

The Ask OSAI box starts with:

```text
whats the update ?
```

When clicked, Rust treats this as a full server overview request.

## What Rust Checks

Rust uses the current scanner snapshot and maps user words to server signals:

| User asks about | Rust insight |
| --- | --- |
| whats the update, overview, health, all, everything | Full server overview |
| cpu, core, processor, load | CPU / Core Utilization |
| memory, ram, swap | Memory / RAM Utilization |
| disk, storage, filesystem, mount, space | Storage / Disk Usage |
| network, port, listening, socket, firewall | Network / Listening Ports |
| process, top, pid | Top Processes |
| service, app, database, postgres, redis, mysql | Services / Apps / Databases |
| finding, warning, critical, issue, problem | Rule Findings |
| kubernetes, k8s, pod, node | Kubernetes Signals |
| gitlab, gitaly, workhorse | GitLab Signals |

## Output Shape

The API returns structured `query_insights`:

```json
{
  "id": "memory",
  "label": "Memory / RAM Utilization",
  "status": "normal use",
  "severity": "ok",
  "summary": "Rust mapped memory/RAM words to RAM and swap usage...",
  "metrics": [],
  "recommendation": "Memory usage is normal..."
}
```

The browser renders each insight as a visual card with:

- status chip
- clickable ask deeper label
- signal label
- metric bars
- recommendation

Clicking an `ask:` label puts that deeper question into the Ask OSAI form and runs it.

## Important Design Rule

Ask OSAI does not need Qwen to answer basic server questions.

```text
Rust scanner data -> deterministic insight cards -> optional Qwen explanation
```

This keeps server status predictable. Qwen is useful for explanation, but Rust owns the facts.
