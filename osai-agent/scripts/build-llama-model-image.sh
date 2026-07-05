#!/usr/bin/env bash
# =============================================================================
# File: scripts/build-llama-model-image.sh
# Purpose:
#   Builds the optional Docker image that contains the local Qwen GGUF model.
#
# Where this fits in OSAI:
#   Wraps docker/llama-model/Dockerfile so repeated server deployment does not require a runtime model download.
#
# Topics to know before editing:
#   Bash safety flags, environment variables, Docker/Cargo commands, and local deployment workflow.
#
# Important operational notes:
#   Fails early if models/$MODEL_FILE is missing because Docker cannot bake a file it cannot see.
# =============================================================================
set -euo pipefail

MODEL_FILE="${MODEL_FILE:-Qwen3-4B-Q4_K_M.gguf}"
IMAGE_NAME="${IMAGE_NAME:-osai-llama-qwen-with-model:local}"
MODEL_PATH="models/${MODEL_FILE}"

if [[ ! -s "${MODEL_PATH}" ]]; then
  echo "[ERROR] Missing model file: ${MODEL_PATH}"
  echo "Put the GGUF there first, then rerun this script."
  exit 1
fi

echo "[INFO] Building ${IMAGE_NAME} with ${MODEL_PATH}"
DOCKER_BUILDKIT=1 docker build \
  -f docker/llama-model/Dockerfile \
  --build-arg "MODEL_FILE=${MODEL_FILE}" \
  -t "${IMAGE_NAME}" \
  .

echo "[OK] Built ${IMAGE_NAME}"
