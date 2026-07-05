# =============================================================================
# File: docker/llama-model/Dockerfile
# Purpose:
#   Builds an optional llama.cpp image that contains the selected Qwen GGUF file.
#
# Where this fits in OSAI:
#   Used by scripts/build-llama-model-image.sh and docker-compose.model-image.yml for baked-model deployments.
#
# Topics to know before editing:
#   OSAI architecture, file format syntax, and deployment workflow.
#
# Important operational notes:
#   The COPY step is the whole point of this file; the GGUF must be present before building the image.
# =============================================================================
FROM ghcr.io/ggml-org/llama.cpp:server

ARG MODEL_FILE=Qwen3-4B-Q4_K_M.gguf
ENV OSAI_MODEL_PATH=/models/${MODEL_FILE}

# This COPY is intentionally only used by the model-image build profile.
# docker/llama-model/Dockerfile.dockerignore allows selected GGUF files into
# this build context while the root .dockerignore still protects normal builds.
COPY models/${MODEL_FILE} /models/${MODEL_FILE}

RUN test -s "/models/${MODEL_FILE}"

# llama.cpp still memory-maps a model that lives inside the container
# filesystem. The benefit is no runtime download and no host model mount.
EXPOSE 8080
