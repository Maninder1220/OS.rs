-- =============================================================================
-- File: storage/migrations/003-rename-minio-columns.sql
-- Purpose:
--   Migration that renames older MinIO-specific columns to generic object-store names.
--
-- Where this fits in OSAI:
--   Keeps older installations compatible with the RustFS/object-store terminology used now.
--
-- Topics to know before editing:
--   PostgreSQL DDL, indexes, constraints, JSONB, and OSAI storage tables.
--
-- Important operational notes:
--   Safe migration scripts should be idempotent because operators may rerun them during upgrades.
-- =============================================================================
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'osai_scan_history'
          AND column_name = 'minio_bucket'
    ) AND NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'osai_scan_history'
          AND column_name = 'object_store_bucket'
    ) THEN
        ALTER TABLE osai_scan_history
            RENAME COLUMN minio_bucket TO object_store_bucket;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'osai_scan_history'
          AND column_name = 'minio_object_key'
    ) AND NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'osai_scan_history'
          AND column_name = 'object_store_key'
    ) THEN
        ALTER TABLE osai_scan_history
            RENAME COLUMN minio_object_key TO object_store_key;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'osai_scan_history'
          AND column_name = 'object_store_bucket'
    ) THEN
        ALTER TABLE osai_scan_history
            ADD COLUMN object_store_bucket TEXT;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'osai_scan_history'
          AND column_name = 'object_store_key'
    ) THEN
        ALTER TABLE osai_scan_history
            ADD COLUMN object_store_key TEXT;
    END IF;
END $$;
