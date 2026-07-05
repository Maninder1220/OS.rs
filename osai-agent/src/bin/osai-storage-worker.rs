// =============================================================================
// File: src/bin/osai-storage-worker.rs
// Purpose:
//   Persistence worker that saves scans to PostgreSQL/RustFS and prepares Markdown memory for Cognee.
//
// Where this fits in OSAI:
//   Runs beside the web server to convert live snapshots into durable history and AI memory.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   This worker is the main bridge from raw machine facts to long-term AI-readable memory.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::time::Duration;

use anyhow::{Context, Result};
use bytes::Bytes;
use clap::Parser;
use object_store::{aws::AmazonS3Builder, path::Path as ObjectPath, ObjectStore, ObjectStoreExt};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_postgres::{Client as PgClient, NoTls};
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "osai-storage-worker")]
#[command(about = "Rust worker that persists OSAI scans to PostgreSQL, RustFS/S3 object storage, and Cognee outbox tables")]
struct Args {
    /// Run one sync cycle and exit.
    #[arg(long)]
    once: bool,

    /// Number of recent scan records to fetch from the Rust agent.
    #[arg(long, default_value_t = 50)]
    history_limit: usize,

    /// Rebuild Markdown memory/outbox rows even when the scan already exists in PostgreSQL.
    #[arg(long)]
    rebuild_memory: bool,
}

#[derive(Debug, Clone)]
struct Settings {
    agent_url: String,
    agent_token: Option<String>,
    postgres_dsn: String,
    object_store_endpoint: String,
    object_store_access_key: String,
    object_store_secret_key: String,
    object_store_bucket: String,
    object_store_secure: bool,
    object_store_region: String,
    cognee_dataset: String,
    cognee_memory_min_interval_seconds: i64,
    cognee_memory_always_on_findings: bool,
    poll_seconds: u64,
}

#[derive(Debug)]
struct PersistOutcome {
    inserted: bool,
    memory_queued: bool,
    memory_reason: String,
}

#[derive(Debug, Clone, Deserialize)]
struct HistorySummary {
    id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryRecord {
    id: String,
    generated_at: String,
    hostname: String,
    snapshot: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "osai_storage_worker=info".to_string()),
        )
        .compact()
        .init();

    let args = Args::parse();
    load_env_files();
    let settings = Settings::from_env();

    info!(
        agent = %settings.agent_url,
        bucket = %settings.object_store_bucket,
        postgres = %redacted_dsn(&settings.postgres_dsn),
        "starting storage worker"
    );

    loop {
        if let Err(err) = run_once(&settings, args.history_limit, args.rebuild_memory).await {
            warn!("sync cycle failed: {err:#}");
        }

        if args.once {
            break;
        }

        tokio::time::sleep(Duration::from_secs(settings.poll_seconds.max(5))).await;
    }

    Ok(())
}

fn load_env_files() {
    let _ = dotenvy::from_filename(".env.storage");
    let _ = dotenvy::from_filename(".env.cognee");
    let _ = dotenvy::dotenv();
}

impl Settings {
    fn from_env() -> Self {
        Self {
            agent_url: env_or("OSAI_AGENT_URL", "http://127.0.0.1:8000").trim_end_matches('/').to_string(),
            agent_token: std::env::var("OSAI_AGENT_TOKEN")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            postgres_dsn: env_or(
                "OSAI_POSTGRES_DSN",
                "postgresql://osai:osai_password@127.0.0.1:5432/osai_agent",
            ),
            object_store_endpoint: env_or_compat("OBJECT_STORE_ENDPOINT", "MINIO_ENDPOINT", "127.0.0.1:9000"),
            object_store_access_key: env_or_compat("OBJECT_STORE_ACCESS_KEY", "MINIO_ACCESS_KEY", "rustfsadmin"),
            object_store_secret_key: env_or_compat("OBJECT_STORE_SECRET_KEY", "MINIO_SECRET_KEY", "rustfsadmin"),
            object_store_bucket: env_or_compat("OBJECT_STORE_BUCKET", "MINIO_BUCKET", "osai-agent"),
            object_store_secure: env_bool_compat("OBJECT_STORE_SECURE", "MINIO_SECURE", false),
            object_store_region: env_or_compat("OBJECT_STORE_REGION", "MINIO_REGION", "us-east-1"),
            cognee_dataset: env_or("COGNEE_DATASET", "osai-agent-memory"),
            cognee_memory_min_interval_seconds: env_i64("OSAI_COGNEE_MEMORY_MIN_INTERVAL_SECONDS", 900).max(60),
            cognee_memory_always_on_findings: env_bool("OSAI_COGNEE_MEMORY_ALWAYS_ON_FINDINGS", true),
            poll_seconds: env_or("OSAI_POLL_SECONDS", "30").parse().unwrap_or(30),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_or_compat(primary: &str, legacy: &str, default: &str) -> String {
    std::env::var(primary)
        .or_else(|_| std::env::var(legacy))
        .unwrap_or_else(|_| default.to_string())
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "y" | "on"))
        .unwrap_or(default)
}

fn env_bool_compat(primary: &str, legacy: &str, default: bool) -> bool {
    std::env::var(primary)
        .or_else(|_| std::env::var(legacy))
        .ok()
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "y" | "on"))
        .unwrap_or(default)
}

fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

async fn run_once(settings: &Settings, history_limit: usize, rebuild_memory: bool) -> Result<()> {
    let agent = AgentClient::new(settings)?;
    let store = build_object_store(settings)?;
    let pg = connect_postgres(settings).await?;

    let mut records = agent.history(history_limit).await?;
    records.reverse();

    for summary in records {
        let exists = scan_exists(&pg, &summary.id).await?;
        if exists && !rebuild_memory {
            continue;
        }

        let record = agent.history_record(&summary.id).await?;
        // Store both machine-readable evidence and human/LLM-readable memory.
        // Raw JSON is the audit trail; Markdown is what Cognee can chunk and
        // recall more naturally during future troubleshooting.
        let object_key = safe_object_key(&record);
        let memory_object_key = safe_memory_object_key(&record);
        let memory_markdown = build_memory_markdown(&record, &settings.object_store_bucket, &object_key, &memory_object_key);

        put_snapshot(store.as_ref(), &object_key, &record).await?;
        put_object_bytes(
            store.as_ref(),
            &memory_object_key,
            Bytes::from(memory_markdown.clone().into_bytes()),
        )
        .await?;

        // PostgreSQL ties together the scan metadata, finding rows, object-store
        // keys, and the Cognee outbox event used by osai-cognee-ingest.
        let outcome = upsert_scan(
            &pg,
            settings,
            &record,
            &object_key,
            &memory_object_key,
            &memory_markdown,
        )
        .await?;
        if outcome.inserted {
            info!(
                scan_id = %record.id,
                raw_object = %format!("s3://{}/{}", settings.object_store_bucket, object_key),
                memory_object = %format!("s3://{}/{}", settings.object_store_bucket, memory_object_key),
                memory_queued = outcome.memory_queued,
                memory_reason = %outcome.memory_reason,
                "stored scan and markdown memory"
            );
        } else if outcome.memory_queued {
            info!(
                scan_id = %record.id,
                memory_reason = %outcome.memory_reason,
                "queued updated markdown memory for Cognee"
            );
        }
    }

    Ok(())
}

struct AgentClient {
    base_url: String,
    client: reqwest::Client,
}

impl AgentClient {
    fn new(settings: &Settings) -> Result<Self> {
        let mut headers = HeaderMap::new();
        if let Some(token) = settings.agent_token.as_deref() {
            headers.insert("x-osai-token", HeaderValue::from_str(token)?);
            headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}"))?);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .build()?;

        Ok(Self {
            base_url: settings.agent_url.clone(),
            client,
        })
    }

    async fn history(&self, limit: usize) -> Result<Vec<HistorySummary>> {
        let url = format!("{}/api/history?limit={}", self.base_url, limit.max(1));
        Ok(self.client.get(url).send().await?.error_for_status()?.json().await?)
    }

    async fn history_record(&self, id: &str) -> Result<HistoryRecord> {
        let url = format!("{}/api/history/{}", self.base_url, id);
        Ok(self.client.get(url).send().await?.error_for_status()?.json().await?)
    }
}

fn build_object_store(settings: &Settings) -> Result<Box<dyn ObjectStore>> {
    let endpoint = if settings.object_store_endpoint.starts_with("http://")
        || settings.object_store_endpoint.starts_with("https://")
    {
        settings.object_store_endpoint.clone()
    } else if settings.object_store_secure {
        format!("https://{}", settings.object_store_endpoint)
    } else {
        format!("http://{}", settings.object_store_endpoint)
    };

    let store = AmazonS3Builder::new()
        .with_endpoint(endpoint)
        .with_bucket_name(&settings.object_store_bucket)
        .with_region(&settings.object_store_region)
        .with_access_key_id(&settings.object_store_access_key)
        .with_secret_access_key(&settings.object_store_secret_key)
        .with_allow_http(!settings.object_store_secure)
        .build()
        .context("failed to configure RustFS/S3 object store")?;

    Ok(Box::new(store))
}

async fn put_snapshot(store: &dyn ObjectStore, object_key: &str, record: &HistoryRecord) -> Result<()> {
    let payload = Bytes::from(serde_json::to_vec_pretty(record)?);
    put_object_bytes(store, object_key, payload).await
}

async fn put_object_bytes(store: &dyn ObjectStore, object_key: &str, payload: Bytes) -> Result<()> {
    let path = ObjectPath::from(object_key);
    store
        .put(&path, payload.into())
        .await
        .with_context(|| format!("failed to upload object key {object_key}"))?;
    Ok(())
}

async fn connect_postgres(settings: &Settings) -> Result<PgClient> {
    let (client, connection) = tokio_postgres::connect(&settings.postgres_dsn, NoTls)
        .await
        .context("failed to connect to PostgreSQL")?;

    tokio::spawn(async move {
        if let Err(err) = connection.await {
            warn!("postgres connection task ended: {err}");
        }
    });

    Ok(client)
}

async fn scan_exists(pg: &PgClient, scan_id: &str) -> Result<bool> {
    Ok(!pg
        .query("SELECT 1 FROM osai_scan_history WHERE id = $1", &[&scan_id])
        .await?
        .is_empty())
}

async fn upsert_scan(
    pg: &PgClient,
    settings: &Settings,
    record: &HistoryRecord,
    object_key: &str,
    memory_object_key: &str,
    memory_markdown: &str,
) -> Result<PersistOutcome> {
    let snapshot = &record.snapshot;
    let host = snapshot.get("host").cloned().unwrap_or_else(|| json!({}));
    let os_info = snapshot.get("os").cloned().unwrap_or_else(|| json!({}));
    let hostname = value_str(&host, "hostname").unwrap_or(&record.hostname);
    let (finding_count, warn_count, critical_count, highest_severity) = finding_counts(snapshot);

    pg.execute(
        r#"
        INSERT INTO osai_hosts(hostname, os_name, os_version, kernel_version, cpu_arch)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT(hostname) DO UPDATE SET
            last_seen_at = now(),
            os_name = EXCLUDED.os_name,
            os_version = EXCLUDED.os_version,
            kernel_version = EXCLUDED.kernel_version,
            cpu_arch = EXCLUDED.cpu_arch
        "#,
        &[
            &hostname,
            &value_str(&os_info, "name"),
            &value_str(&os_info, "long_version"),
            &value_str(&os_info, "kernel_version"),
            &value_str(&os_info, "cpu_arch"),
        ],
    )
    .await?;

    let already_exists = !pg
        .query("SELECT 1 FROM osai_scan_history WHERE id = $1", &[&record.id])
        .await?
        .is_empty();

    pg.execute(
        r#"
        INSERT INTO osai_scan_history(
            id, generated_at, hostname, finding_count, warn_count, critical_count,
            highest_severity, snapshot_json, object_store_bucket, object_store_key
        )
        VALUES ($1, $2::text::timestamptz, $3, $4, $5, $6, $7, $8::jsonb, $9, $10)
        ON CONFLICT(id) DO UPDATE SET
            snapshot_json = EXCLUDED.snapshot_json,
            object_store_bucket = EXCLUDED.object_store_bucket,
            object_store_key = EXCLUDED.object_store_key
        "#,
        &[
            &record.id,
            &record.generated_at,
            &hostname,
            &(finding_count as i32),
            &(warn_count as i32),
            &(critical_count as i32),
            &highest_severity,
            snapshot,
            &settings.object_store_bucket,
            &object_key,
        ],
    )
    .await?;

    if let Some(findings) = snapshot.get("findings").and_then(Value::as_array) {
        for finding in findings {
            let evidence = finding.get("evidence").cloned().unwrap_or_else(|| json!([]));
            pg.execute(
                r#"
                INSERT INTO osai_findings(
                    scan_id, rule_id, severity, category, title, detail, recommendation,
                    requires_approval, command_suggestion, evidence
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb)
                ON CONFLICT(scan_id, rule_id, title) DO UPDATE SET
                    severity = EXCLUDED.severity,
                    detail = EXCLUDED.detail,
                    recommendation = EXCLUDED.recommendation,
                    evidence = EXCLUDED.evidence
                "#,
                &[
                    &record.id,
                    &value_str(finding, "rule_id"),
                    &value_str_or(finding, "severity", "info"),
                    &value_str(finding, "category"),
                    &value_str_or(finding, "title", "untitled finding"),
                    &value_str(finding, "detail"),
                    &value_str(finding, "recommendation"),
                    &value_bool(finding, "requires_approval"),
                    &value_str(finding, "command_suggestion"),
                    &evidence,
                ],
            )
            .await?;
        }
    }

    let memory_signature = memory_signature(snapshot, &highest_severity);
    let memory_decision = should_queue_cognee_memory(
        pg,
        settings,
        hostname,
        &highest_severity,
        &memory_signature,
    )
    .await?;

    let memory_text = memory_markdown.to_string();
    let hash = sha256_hex(&memory_text);
    let metadata = json!({
        "bucket": settings.object_store_bucket,
        "hostname": hostname,
        "highest_severity": highest_severity,
        "finding_count": finding_count,
        "memory_signature": memory_signature,
        "memory_reason": memory_decision.reason.clone(),
        "raw_snapshot_object_key": object_key,
        "memory_markdown_object_key": memory_object_key,
        "memory_format": "text/markdown",
        "producer": "osai-storage-worker-rust"
    });

    if memory_decision.queue {
        pg.execute(
            r#"
            INSERT INTO osai_cognee_outbox(scan_id, dataset_name, content_hash)
            VALUES ($1, $2, $3)
            ON CONFLICT(scan_id) DO UPDATE SET
                dataset_name = EXCLUDED.dataset_name,
                content_hash = EXCLUDED.content_hash,
                status = CASE
                    WHEN osai_cognee_outbox.content_hash IS DISTINCT FROM EXCLUDED.content_hash THEN 'pending'
                    ELSE osai_cognee_outbox.status
                END,
                last_error = CASE
                    WHEN osai_cognee_outbox.content_hash IS DISTINCT FROM EXCLUDED.content_hash THEN NULL
                    ELSE osai_cognee_outbox.last_error
                END,
                updated_at = now(),
                ingested_at = CASE
                    WHEN osai_cognee_outbox.content_hash IS DISTINCT FROM EXCLUDED.content_hash THEN NULL
                    ELSE osai_cognee_outbox.ingested_at
                END
            "#,
            &[&record.id, &settings.cognee_dataset, &hash],
        )
        .await?;

        pg.execute(
            r#"
            INSERT INTO osai_memory_events(event_type, source_id, dataset_name, content, content_hash, metadata)
            VALUES ($1, $2, $3, $4, $5, $6::jsonb)
            ON CONFLICT(content_hash) DO NOTHING
            "#,
            &[
                &"server_scan_summary_markdown",
                &record.id,
                &settings.cognee_dataset,
                &memory_text,
                &hash,
                &metadata,
            ],
        )
        .await?;
    }

    Ok(PersistOutcome {
        inserted: !already_exists,
        memory_queued: memory_decision.queue,
        memory_reason: memory_decision.reason,
    })
}

#[derive(Debug)]
struct MemoryDecision {
    queue: bool,
    reason: String,
}

async fn should_queue_cognee_memory(
    pg: &PgClient,
    settings: &Settings,
    hostname: &str,
    highest_severity: &str,
    signature: &str,
) -> Result<MemoryDecision> {
    let row = pg
        .query_opt(
            r#"
            SELECT
                metadata->>'memory_signature',
                metadata->>'highest_severity',
                created_at <= now() - ($3::bigint * interval '1 second') AS interval_elapsed
            FROM osai_memory_events
            WHERE event_type = 'server_scan_summary_markdown'
              AND dataset_name = $1
              AND metadata->>'hostname' = $2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            &[&settings.cognee_dataset, &hostname, &settings.cognee_memory_min_interval_seconds],
        )
        .await?;

    let Some(row) = row else {
        return Ok(MemoryDecision {
            queue: true,
            reason: "first_memory_for_host".to_string(),
        });
    };

    let previous_signature: Option<String> = row.get(0);
    let previous_severity: Option<String> = row.get(1);
    let interval_elapsed: bool = row.get(2);

    if previous_signature.as_deref() != Some(signature) {
        return Ok(MemoryDecision {
            queue: true,
            reason: "server_state_changed".to_string(),
        });
    }

    if previous_severity.as_deref() != Some(highest_severity) {
        return Ok(MemoryDecision {
            queue: true,
            reason: "severity_changed".to_string(),
        });
    }

    if interval_elapsed {
        let reason = if settings.cognee_memory_always_on_findings
            && matches!(highest_severity, "critical" | "warn")
        {
            "important_finding_periodic_refresh".to_string()
        } else {
            format!(
                "periodic_summary_after_{}_seconds",
                settings.cognee_memory_min_interval_seconds
            )
        };
        return Ok(MemoryDecision {
            queue: true,
            reason,
        });
    }

    Ok(MemoryDecision {
        queue: false,
        reason: "unchanged_recent_scan".to_string(),
    })
}

fn memory_signature(snapshot: &Value, highest_severity: &str) -> String {
    let memory = snapshot.get("memory").unwrap_or(&Value::Null);
    let compute = snapshot.get("compute").unwrap_or(&Value::Null);
    let kubernetes = snapshot.get("kubernetes").unwrap_or(&Value::Null);
    let gitlab = snapshot.get("gitlab").unwrap_or(&Value::Null);
    let findings = snapshot
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let finding_keys = findings
        .iter()
        .map(|finding| {
            format!(
                "{}:{}:{}",
                value_str_or(finding, "severity", "info"),
                value_str_or(finding, "rule_id", "unknown"),
                value_str_or(finding, "title", "untitled")
            )
        })
        .collect::<Vec<_>>();

    let signature = json!({
        "highest_severity": highest_severity,
        "finding_keys": finding_keys,
        "memory_used_bucket": percent_bucket(
            value_u64(memory, "used_bytes"),
            value_u64(memory, "total_bytes"),
        ),
        "cpu_bucket": numeric_bucket(value_f64(compute, "global_cpu_usage_percent")),
        "kubernetes_detected": value_bool(kubernetes, "detected"),
        "gitlab_detected": value_bool(gitlab, "detected"),
    });

    sha256_hex(&signature.to_string())
}

fn percent_bucket(used: Option<u64>, total: Option<u64>) -> String {
    match (used, total) {
        (Some(used), Some(total)) if total > 0 => numeric_bucket(Some((used as f64 / total as f64) * 100.0)),
        _ => "unknown".to_string(),
    }
}

fn numeric_bucket(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "unknown".to_string();
    };
    if value >= 95.0 {
        "critical_95_plus"
    } else if value >= 85.0 {
        "high_85_plus"
    } else if value >= 70.0 {
        "attention_70_plus"
    } else {
        "normal"
    }
    .to_string()
}

fn safe_object_key(record: &HistoryRecord) -> String {
    let safe_host = safe_hostname(record);
    let safe_time = record.generated_at.replace(':', "-").replace('+', "Z");
    format!("snapshots/{safe_host}/{safe_time}/{}.json", record.id)
}

fn safe_memory_object_key(record: &HistoryRecord) -> String {
    let safe_host = safe_hostname(record);
    let safe_time = record.generated_at.replace(':', "-").replace('+', "Z");
    format!("memory/scans/{safe_host}/{safe_time}/{}.md", record.id)
}

fn safe_hostname(record: &HistoryRecord) -> String {
    let hostname = record
        .snapshot
        .get("host")
        .and_then(|host| value_str(host, "hostname"))
        .unwrap_or(&record.hostname);
    hostname
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '-' })
        .collect::<String>()
}

fn finding_counts(snapshot: &Value) -> (usize, usize, usize, String) {
    let findings = snapshot
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let warn_count = findings
        .iter()
        .filter(|finding| value_str_or(finding, "severity", "") == "warn")
        .count();
    let critical_count = findings
        .iter()
        .filter(|finding| value_str_or(finding, "severity", "") == "critical")
        .count();
    let highest = if critical_count > 0 {
        "critical"
    } else if warn_count > 0 {
        "warn"
    } else if findings.is_empty() {
        "ok"
    } else {
        "info"
    };
    (findings.len(), warn_count, critical_count, highest.to_string())
}

fn build_memory_markdown(
    record: &HistoryRecord,
    bucket: &str,
    raw_object_key: &str,
    memory_object_key: &str,
) -> String {
    let snapshot = &record.snapshot;
    let host = snapshot.get("host").unwrap_or(&Value::Null);
    let os_info = snapshot.get("os").unwrap_or(&Value::Null);
    let memory = snapshot.get("memory").unwrap_or(&Value::Null);
    let compute = snapshot.get("compute").unwrap_or(&Value::Null);
    let disks = snapshot.get("disks").unwrap_or(&Value::Null);
    let processes = snapshot.get("processes").unwrap_or(&Value::Null);
    let ports = snapshot.get("ports").unwrap_or(&Value::Null);
    let kubernetes = snapshot.get("kubernetes").unwrap_or(&Value::Null);
    let gitlab = snapshot.get("gitlab").unwrap_or(&Value::Null);
    let findings = snapshot
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let hostname = value_str(host, "hostname").unwrap_or(&record.hostname);
    let (finding_count, warn_count, critical_count, highest_severity) = finding_counts(snapshot);

    let memory_used = value_u64(memory, "used_bytes");
    let memory_total = value_u64(memory, "total_bytes");
    let memory_available = value_u64(memory, "available_bytes");
    let memory_used_percent = percent(memory_used, memory_total);

    let mut doc = String::new();
    push_line(&mut doc, "# OSAI Server Scan Summary");
    push_line(&mut doc, "");
    push_line(&mut doc, "> This file is generated by the Rust storage worker. It is intentionally descriptive Markdown so humans, Cognee, pgvector, Kuzu, and Qwen can understand the scan without reading noisy raw JSON.");
    push_line(&mut doc, "");

    push_line(&mut doc, "## Identity");
    push_line(&mut doc, &format!("- **Host:** `{}`", hostname));
    push_line(&mut doc, &format!("- **Scan ID:** `{}`", record.id));
    push_line(&mut doc, &format!("- **Generated at:** `{}`", record.generated_at));
    push_line(&mut doc, &format!("- **Dataset target:** `osai-agent-memory`"));
    push_line(&mut doc, "");

    push_line(&mut doc, "## System Facts");
    push_line(&mut doc, &format!("- **OS:** {}", value_str(os_info, "long_version").or_else(|| value_str(os_info, "name")).unwrap_or("unknown")));
    push_line(&mut doc, &format!("- **Kernel:** {}", value_str(os_info, "kernel_long_version").or_else(|| value_str(os_info, "kernel_version")).unwrap_or("unknown")));
    push_line(&mut doc, &format!("- **Architecture:** {}", value_str(os_info, "cpu_arch").unwrap_or("unknown")));
    push_line(&mut doc, &format!("- **Logical CPUs:** {}", value_number(compute, "logical_cpus")));
    push_line(&mut doc, &format!("- **Current CPU usage:** {}%", value_number(compute, "global_cpu_usage_percent")));
    push_line(&mut doc, &format!("- **Memory used:** {} of {} bytes{}", memory_used.map_or("unknown".to_string(), human_bytes), memory_total.map_or("unknown".to_string(), human_bytes), memory_used_percent.map(|p| format!(" ({p:.1}%)")).unwrap_or_default()));
    push_line(&mut doc, &format!("- **Memory available:** {}", memory_available.map_or("unknown".to_string(), human_bytes)));
    push_line(&mut doc, "");

    push_line(&mut doc, "## Current Status Summary");
    push_line(&mut doc, &format!("The host `{}` was scanned at `{}`. The rule engine reported **{}** findings. Highest severity is **{}**. Warnings: **{}**. Critical findings: **{}**.", hostname, record.generated_at, finding_count, highest_severity, warn_count, critical_count));
    if value_bool(kubernetes, "detected") {
        push_line(&mut doc, &format!("Kubernetes was detected. {}", value_str_or(kubernetes, "summary", "")));
    } else {
        push_line(&mut doc, "Kubernetes was not detected by this scan.");
    }
    if value_bool(gitlab, "detected") {
        push_line(&mut doc, &format!("GitLab was detected. {}", value_str_or(gitlab, "summary", "")));
    } else {
        push_line(&mut doc, "GitLab was not detected by this scan.");
    }
    push_line(&mut doc, "");

    push_line(&mut doc, "## Findings And Recommendations");
    if findings.is_empty() {
        push_line(&mut doc, "- No current findings were detected by the rule engine.");
    } else {
        for finding in findings.iter().take(30) {
            push_line(&mut doc, &format!("### {}", value_str_or(finding, "title", "Finding")));
            push_line(&mut doc, &format!("- **Severity:** {}", value_str_or(finding, "severity", "info")));
            push_line(&mut doc, &format!("- **Category:** {}", value_str_or(finding, "category", "unknown")));
            push_line(&mut doc, &format!("- **Detail:** {}", value_str_or(finding, "detail", "")));
            push_line(&mut doc, &format!("- **Recommendation:** {}", value_str_or(finding, "recommendation", "")));
            if let Some(command) = value_str(finding, "command_suggestion") {
                if !command.trim().is_empty() {
                    push_line(&mut doc, &format!("- **Suggested read-only command:** `{}`", command));
                }
            }
            push_line(&mut doc, "");
        }
    }

    push_line(&mut doc, "## Runtime Signals");
    push_line(&mut doc, "### Disks");
    push_list_preview(&mut doc, disks, 10, |disk| {
        format!(
            "{} mounted at {} has {} used of {} bytes",
            value_str_or(disk, "name", "disk"),
            value_str_or(disk, "mount_point", "unknown"),
            value_number(disk, "used_bytes"),
            value_number(disk, "total_bytes")
        )
    });
    push_line(&mut doc, "");

    push_line(&mut doc, "### Listening Ports");
    push_list_preview(&mut doc, ports, 15, |port| {
        format!(
            "{}:{} via {} process={}",
            value_str_or(port, "local_address", "0.0.0.0"),
            value_number(port, "local_port"),
            value_str_or(port, "protocol", "tcp"),
            value_str_or(port, "process_name", "unknown")
        )
    });
    push_line(&mut doc, "");

    push_line(&mut doc, "### Top Processes");
    push_list_preview(&mut doc, processes, 15, |process| {
        format!(
            "pid={} name={} cpu={} memory_bytes={}",
            value_number(process, "pid"),
            value_str_or(process, "name", "unknown"),
            value_number(process, "cpu_usage_percent"),
            value_number(process, "memory_bytes")
        )
    });
    push_line(&mut doc, "");

    push_line(&mut doc, "## Evidence Links");
    push_line(&mut doc, &format!("- **Raw JSON snapshot:** `s3://{}/{}`", bucket, raw_object_key));
    push_line(&mut doc, &format!("- **This Markdown memory:** `s3://{}/{}`", bucket, memory_object_key));
    push_line(&mut doc, &format!("- **PostgreSQL scan table:** `osai_scan_history.id = {}`", record.id));
    push_line(&mut doc, "");

    push_line(&mut doc, "## Tags");
    let mut tags = vec!["osai", "server-scan", "linux", hostname];
    if value_bool(kubernetes, "detected") { tags.push("kubernetes"); }
    if value_bool(gitlab, "detected") { tags.push("gitlab"); }
    if critical_count > 0 { tags.push("critical"); }
    else if warn_count > 0 { tags.push("warning"); }
    push_line(&mut doc, &tags.iter().map(|tag| format!("`{}`", tag)).collect::<Vec<_>>().join(", "));
    push_line(&mut doc, "");

    push_line(&mut doc, "## Safety Note");
    push_line(&mut doc, "This memory is evidence and context. It is not an instruction to execute repair commands. OSAI must ask for approval before any repair action.");

    doc
}

fn push_line(doc: &mut String, line: &str) {
    doc.push_str(line);
    doc.push('\n');
}

fn push_list_preview<F>(doc: &mut String, value: &Value, limit: usize, render: F)
where
    F: Fn(&Value) -> String,
{
    match value.as_array() {
        Some(items) if !items.is_empty() => {
            for item in items.iter().take(limit) {
                push_line(doc, &format!("- {}", render(item)));
            }
        }
        _ => push_line(doc, "- No data captured in this section."),
    }
}

fn value_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn value_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn percent(part: Option<u64>, total: Option<u64>) -> Option<f64> {
    let part = part?;
    let total = total?;
    if total == 0 { None } else { Some((part as f64 / total as f64) * 100.0) }
}

fn human_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = value as f64;
    let mut unit = 0usize;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.2} {}", UNITS[unit])
}

fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn value_str_or<'a>(value: &'a Value, key: &str, default: &'a str) -> &'a str {
    value_str(value, key).unwrap_or(default)
}

fn value_bool(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn value_number(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(Value::to_string)
        .unwrap_or_else(|| "unknown".to_string())
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

fn redacted_dsn(dsn: &str) -> String {
    let Some((scheme, rest)) = dsn.split_once("://") else {
        return "<redacted>".to_string();
    };
    let Some((_, host_part)) = rest.rsplit_once('@') else {
        return format!("{scheme}://<redacted>");
    };
    format!("{scheme}://<redacted>@{host_part}")
}
