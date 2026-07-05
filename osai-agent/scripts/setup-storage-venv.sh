#!/usr/bin/env bash
# =============================================================================
# File: scripts/setup-storage-venv.sh
# Purpose:
#   Legacy helper kept for older storage/Cognee setup experiments.
#
# Where this fits in OSAI:
#   Documented as deprecated because Docker Compose is now the expected Cognee path.
#
# Topics to know before editing:
#   Bash safety flags, environment variables, Docker/Cargo commands, and local deployment workflow.
#
# Important operational notes:
#   Prefer Docker Compose for current phase18 workflows.
# =============================================================================
set -euo pipefail

echo "Deprecated: this project no longer uses a host .venv-storage folder."
echo "Use Docker Compose for Cognee instead:"
echo "  docker compose -f docker-compose.storage.yml up -d --build"
echo "Rust binaries handle OSAI storage and Cognee HTTP ingestion."
