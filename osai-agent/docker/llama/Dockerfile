# =============================================================================
# File: docker/llama/Dockerfile
# Purpose:
#   Defines the small llama.cpp server image used when the GGUF is mounted from the host.
#
# Where this fits in OSAI:
#   Used by docker-compose.storage.yml for fast rebuilds and local development.
#
# Topics to know before editing:
#   OSAI architecture, file format syntax, and deployment workflow.
#
# Important operational notes:
#   The model is not copied here; the compose volume provides /models at runtime.
# =============================================================================
FROM ghcr.io/ggml-org/llama.cpp:server

# This image provides the llama.cpp server binary only.
# The GGUF model is mounted from ./models at runtime so image rebuilds do not
# copy a multi-GB model and Docker layer cache remains useful.
EXPOSE 8080
