# =============================================================================
# File: docker/cognee/Dockerfile
# Purpose:
#   Builds the local Cognee REST service image used for memory ingestion and recall.
#
# Where this fits in OSAI:
#   Used by both Docker Compose modes when running Cognee locally instead of only Cognee Cloud.
#
# Topics to know before editing:
#   OSAI architecture, file format syntax, and deployment workflow.
#
# Important operational notes:
#   Keep Python/Cognee dependency choices stable because memory ingestion depends on this service starting cleanly.
# =============================================================================
FROM python:3.11-slim

ENV PYTHONDONTWRITEBYTECODE=1
ENV PYTHONUNBUFFERED=1
ENV PIP_NO_CACHE_DIR=1

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN python -m pip install --upgrade pip wheel setuptools \
    && python -m pip install "cognee[postgres-binary,fastembed]" uvicorn[standard]

EXPOSE 8001

CMD ["python", "-m", "uvicorn", "cognee.api.client:app", "--host", "0.0.0.0", "--port", "8001"]
