-- =============================================================================
-- File: storage/postgres-init/002-osai-schema.sql
-- Purpose:
--   PostgreSQL schema for scan history, findings, object-store pointers, and Cognee memory queues.
--
-- Where this fits in OSAI:
--   Creates the durable OSAI operational database used by storage, ingest, and Ask OSAI.
--
-- Topics to know before editing:
--   PostgreSQL DDL, indexes, constraints, JSONB, and OSAI storage tables.
--
-- Important operational notes:
--   Schema changes must stay compatible with osai-storage-worker, osai-cognee-ingest, and Ask OSAI queries.
-- =============================================================================
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS osai_hosts (
    hostname TEXT PRIMARY KEY,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    os_name TEXT,
    os_version TEXT,
    kernel_version TEXT,
    cpu_arch TEXT
);

CREATE TABLE IF NOT EXISTS osai_scan_history (
    id TEXT PRIMARY KEY,
    generated_at TIMESTAMPTZ NOT NULL,
    hostname TEXT NOT NULL REFERENCES osai_hosts(hostname) ON UPDATE CASCADE,
    finding_count INTEGER NOT NULL DEFAULT 0,
    warn_count INTEGER NOT NULL DEFAULT 0,
    critical_count INTEGER NOT NULL DEFAULT 0,
    highest_severity TEXT NOT NULL,
    snapshot_json JSONB NOT NULL,
    object_store_bucket TEXT,
    object_store_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_osai_scan_history_host_time
    ON osai_scan_history(hostname, generated_at DESC);

CREATE INDEX IF NOT EXISTS idx_osai_scan_history_snapshot_gin
    ON osai_scan_history USING GIN (snapshot_json);

CREATE TABLE IF NOT EXISTS osai_findings (
    id BIGSERIAL PRIMARY KEY,
    scan_id TEXT NOT NULL REFERENCES osai_scan_history(id) ON DELETE CASCADE,
    rule_id TEXT,
    severity TEXT NOT NULL,
    category TEXT,
    title TEXT NOT NULL,
    detail TEXT,
    recommendation TEXT,
    requires_approval BOOLEAN NOT NULL DEFAULT false,
    command_suggestion TEXT,
    evidence JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(scan_id, rule_id, title)
);

CREATE INDEX IF NOT EXISTS idx_osai_findings_scan_id
    ON osai_findings(scan_id);

CREATE INDEX IF NOT EXISTS idx_osai_findings_severity
    ON osai_findings(severity);

CREATE TABLE IF NOT EXISTS osai_cognee_outbox (
    id BIGSERIAL PRIMARY KEY,
    scan_id TEXT NOT NULL UNIQUE REFERENCES osai_scan_history(id) ON DELETE CASCADE,
    dataset_name TEXT NOT NULL DEFAULT 'osai-agent-memory',
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    content_hash TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ingested_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_osai_cognee_outbox_status
    ON osai_cognee_outbox(status, created_at);

CREATE TABLE IF NOT EXISTS osai_memory_events (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    source_id TEXT NOT NULL,
    dataset_name TEXT NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL UNIQUE,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ingested_at TIMESTAMPTZ
);

GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO osai;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO osai;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO osai;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO osai;
