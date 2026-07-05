# Ask OSAI Rust Insights

> File guide:
> - Purpose: Documents how Ask OSAI uses Rust-collected insights as factual grounding.
> - Where this fits in OSAI: Supports developers changing ask.rs, osai-ask.rs, and UI ask flows.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Avoid implying Qwen can inspect systems directly; Rust provides facts first.



This phase adds a deterministic Rust understanding layer before Qwen.

## Goal

When a user types a question such as:

```text
Status of my core and memory
```

Rust should first understand the important words, match them with current scan data, and return clear server status cards.

## Current Flow

```text
Browser Ask OSAI form
  -> POST /api/ask
  -> Rust reads the question
  -> Rust matches known words
  -> Rust reads current scanner snapshot
  -> Rust creates query_insights
  -> Browser renders visual cards
  -> Qwen is used only if available
```

## Example Word Mapping

| User words | Rust signal | Scanner fields |
| --- | --- | --- |
| core, cores, cpu, processor, load | CPU / Core Utilization | compute.global_cpu_usage_percent, compute.logical_cpus, compute.physical_cores, compute.cpus |
| memory, ram, swap | Memory / RAM Utilization | memory.used_bytes, memory.total_bytes, memory.available_bytes, memory.used_swap_bytes |

## Status Rules

| Usage | Status | Severity |
| --- | --- | --- |
| 0-34.9% | low use | ok |
| 35-69.9% | normal use | ok |
| 70-84.9% | needs attention | warn |
| 85-100% | high use | critical |

## Why This Is Better

- Rust gives predictable answers from exact scan data.
- The dashboard works even if Qwen, Cognee, or PostgreSQL are down.
- Qwen can later explain the same evidence, but it does not invent metrics.
- The browser gets structured JSON, so it can show clean visual cards.

## Files Changed

| File | Purpose |
| --- | --- |
| `src/intent.rs` | New deterministic word-to-signal analyzer |
| `src/ask.rs` | Adds `query_insights` to Ask OSAI response and deterministic fallback |
| `src/main.rs` | Loads the new `intent` module |
| `web/app.js` | Renders insight cards in Ask OSAI output |
| `web/app.css` | Styles the insight cards and metric bars |

## Important Note

This phase does not run shell commands from Ask OSAI. It only uses the scan data that Rust already collected.

Command execution should stay separate and guarded through the existing action workflow.
