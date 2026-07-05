# Ask OSAI Operator UI

> File guide:
> - Purpose: Explains the browser UI toggle that exposes AI/Ask OSAI behavior to operators.
> - Where this fits in OSAI: UI design note for the dashboard-facing AI controls.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Keep wording aligned with actual app.js toggle behavior.



The dashboard now starts in a quiet Rust-first mode.

## AI Toggle

- `AI off` means Ask OSAI uses deterministic Rust scanner logic only.
- `AI requested` means the next Ask OSAI request will try the inference layer.
- `AI ready` is green only after llama.cpp/Qwen actually refined the answer.
- `AI not used` means Rust fallback answered because AI was unavailable, unhealthy, or failed.

The browser sends `use_ai: true` only when the operator turns AI on. When AI is off, Rust skips llama/Qwen, Cognee recall, and PostgreSQL context loading for a faster local answer.

## Important Signals

Warning and critical Ask OSAI insight cards are pinned so they remain visible after the next question. Green/OK cards are treated as temporary and disappear when the next answer replaces them.

## Add Server Views

Compute, storage, network/ports, top processes, apps/databases, and findings are hidden by default. Use `Add Server Views` to show only the detail panels needed for the current diagnosis.

The knowledge base is still available to the backend, but it is no longer shown on the dashboard.
