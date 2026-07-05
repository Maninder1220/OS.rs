#!/usr/bin/env bash
# =============================================================================
# File: storage/postgres-init/001-create-databases.sh
# Purpose:
#   PostgreSQL container init script that creates OSAI and Cognee databases/users.
#
# Where this fits in OSAI:
#   Runs automatically on first Postgres volume initialization through Docker entrypoint hooks.
#
# Topics to know before editing:
#   Bash safety flags, environment variables, Docker/Cargo commands, and local deployment workflow.
#
# Important operational notes:
#   Only runs when the database volume is new; existing volumes will not rerun this script.
# =============================================================================
set -euo pipefail

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<'EOSQL'
CREATE USER osai WITH PASSWORD 'osai_password';
CREATE USER cognee WITH PASSWORD 'cognee_password';
CREATE DATABASE osai_agent OWNER osai;
CREATE DATABASE cognee_db OWNER cognee;
EOSQL

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname osai_agent -f /docker-entrypoint-initdb.d/002-osai-schema.sql

psql -v ON_ERROR_STOP=0 --username "$POSTGRES_USER" --dbname cognee_db <<'EOSQL'
CREATE EXTENSION IF NOT EXISTS vector;
EOSQL
