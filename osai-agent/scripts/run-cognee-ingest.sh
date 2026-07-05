#!/usr/bin/env bash
# =============================================================================
# File: scripts/run-cognee-ingest.sh
# Purpose:
#   Runs the Rust Cognee ingestion worker with the expected .env files loaded.
#
# Where this fits in OSAI:
#   Moves pending Markdown memory rows from PostgreSQL into Cognee.
#
# Topics to know before editing:
#   Bash safety flags, environment variables, Docker/Cargo commands, and local deployment workflow.
#
# Important operational notes:
#   Cognee and PostgreSQL must be reachable before this script can succeed.
# =============================================================================
set -euo pipefail

cd "$(dirname "$0")/.."
cargo run --bin osai-cognee-ingest -- "$@"
