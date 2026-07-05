#!/usr/bin/env bash
# =============================================================================
# File: scripts/run-memory-worker.sh
# Purpose:
#   Runs the Rust storage worker with environment loaded from local .env files.
#
# Where this fits in OSAI:
#   Persists scan snapshots, RustFS objects, findings, and Cognee memory outbox rows.
#
# Topics to know before editing:
#   Bash safety flags, environment variables, Docker/Cargo commands, and local deployment workflow.
#
# Important operational notes:
#   The OSAI web server, PostgreSQL, and RustFS should already be running.
# =============================================================================
set -euo pipefail

cd "$(dirname "$0")/.."
cargo run --bin osai-storage-worker -- "$@"
