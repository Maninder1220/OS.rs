#!/usr/bin/env bash
# =============================================================================
# File: scripts/ask-local-qwen.sh
# Purpose:
#   Sends a quick local test request directly to the llama.cpp OpenAI-compatible API.
#
# Where this fits in OSAI:
#   Used to verify the inference layer independently before testing full Ask OSAI behavior.
#
# Topics to know before editing:
#   Bash safety flags, environment variables, Docker/Cargo commands, and local deployment workflow.
#
# Important operational notes:
#   This bypasses Cognee and PostgreSQL; it only checks whether llama.cpp/Qwen is responding.
# =============================================================================
set -euo pipefail

cd "$(dirname "$0")/.."
cargo run --bin osai-ask -- "$@"
