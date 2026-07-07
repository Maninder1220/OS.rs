// =============================================================================
// File: src/bin/osai-cognee-ingest.rs
// Purpose:
//   Cognee ingestion worker that uploads pending Markdown memory rows from PostgreSQL to Cognee REST.
//
// Where this fits in OSAI:
//   Consumes osai_cognee_outbox rows created by the storage worker.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Use bounded batch sizes and retry metadata to avoid flooding Cognee or losing failure context.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    multipart, Client,
};
use tokio_postgres::{Client as PgClient, NoTls};
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "osai-cognee-ingest")]
#[command(about = "Push pending OSAI memory events from PostgreSQL outbox into a local Cognee REST API")]
struct Args {
    /// Run one ingestion cycle and exit.
    #[arg(long)]
    once: bool,

    /// Number of pending outbox rows to ingest per cycle.
    #[arg(long, default_value_t = 10)]
    limit: i64,
}

#[derive(Debug, Clone)]
struct Settings {
    postgres_dsn: String,
    cognee_api_url: String,
    cognee_api_prefix: String,
    cognee_api_key: Option<String>,
    cognee_tenant_id: Option<String>,
    cognee_user_id: Option<String>,
    cognee_send_identity_headers: bool,
    cognee_send_bearer_auth: bool,
    cognee_run_in_background: bool,
    cognee_chunks_per_batch: String,
    cognee_http_timeout_seconds: u64,
    poll_seconds: u64,
}

#[derive(Debug)]
struct PendingMemory {
    outbox_id: i64,
    scan_id: String,
    dataset_name: String,
    content: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "osai_cognee_ingest=info".to_string()),
        )
        .compact()
        .init();

    let args = Args::parse();
    load_env_files();
    let settings = Settings::from_env();

    info!(
        cognee = %settings.cognee_api_url,
        postgres = %redacted_dsn(&settings.postgres_dsn),
        "starting Cognee ingestion bridge"
    );

    loop {
        if let Err(err) = run_once(&settings, args.limit.max(1)).await {
            warn!("Cognee ingestion cycle failed: {err:#}");
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
            postgres_dsn: env_or(
                "OSAI_POSTGRES_DSN",
                "postgresql://osai:osai_password@127.0.0.1:5432/osai_agent",
            ),
            cognee_api_url: env_or("COGNEE_API_URL", "http://127.0.0.1:8001")
                .trim_end_matches('/')
                .to_string(),
            cognee_api_prefix: normalize_api_prefix(&env_or("COGNEE_API_PREFIX", "/api/v1")),
            cognee_api_key: std::env::var("COGNEE_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            cognee_tenant_id: env_optional("COGNEE_TENANT_ID"),
            cognee_user_id: env_optional("COGNEE_USER_ID"),
            cognee_send_identity_headers: env_bool("OSAI_COGNEE_SEND_IDENTITY_HEADERS", false),
            cognee_send_bearer_auth: env_bool("OSAI_COGNEE_SEND_BEARER_AUTH", false),
            cognee_run_in_background: env_bool("OSAI_COGNEE_RUN_IN_BACKGROUND", true),
            cognee_chunks_per_batch: env_or("OSAI_COGNEE_CHUNKS_PER_BATCH", "4"),
            cognee_http_timeout_seconds: env_u64("OSAI_COGNEE_HTTP_TIMEOUT_SECONDS", 120),
            poll_seconds: env_u64("OSAI_COGNEE_POLL_SECONDS", 60),
        }
    }
}

async fn run_once(settings: &Settings, limit: i64) -> Result<()> {
    let pg = connect_postgres(settings).await?;
    let cognee = build_http_client(settings)?;
    let pending = load_pending(&pg, limit).await?;

    if pending.is_empty() {
        info!("no pending Cognee memory rows");
        return Ok(());
    }

    for memory in pending {
        match send_to_cognee(settings, &cognee, &memory).await {
            Ok(()) => {
                mark_ingested(&pg, memory.outbox_id).await?;
                info!(
                    outbox_id = memory.outbox_id,
                    scan_id = %memory.scan_id,
                    dataset = %memory.dataset_name,
                    "ingested memory into Cognee"
                );
            }
            Err(err) => {
                mark_failed(&pg, memory.outbox_id, &err.to_string()).await?;
                warn!(
                    outbox_id = memory.outbox_id,
                    scan_id = %memory.scan_id,
                    "failed to ingest memory into Cognee: {err:#}"
                );
            }
        }
    }

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

fn build_http_client(settings: &Settings) -> Result<Client> {
    let mut headers = HeaderMap::new();
    if let Some(api_key) = settings.cognee_api_key.as_deref() {
        headers.insert("x-api-key", HeaderValue::from_str(api_key)?);
        if settings.cognee_send_bearer_auth {
            headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {api_key}"))?);
        }
    }
    if settings.cognee_send_identity_headers {
        if let Some(tenant_id) = settings.cognee_tenant_id.as_deref() {
            headers.insert("x-cognee-tenant-id", HeaderValue::from_str(tenant_id)?);
        }
        if let Some(user_id) = settings.cognee_user_id.as_deref() {
            headers.insert("x-cognee-user-id", HeaderValue::from_str(user_id)?);
        }
    }

    Ok(Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(settings.cognee_http_timeout_seconds.max(10)))
        .build()?)
}

async fn load_pending(pg: &PgClient, limit: i64) -> Result<Vec<PendingMemory>> {
    let rows = pg
        .query(
            r#"
            SELECT o.id, o.scan_id, o.dataset_name, e.content
            FROM osai_cognee_outbox o
            JOIN osai_memory_events e ON e.content_hash = o.content_hash
            WHERE o.status IN ('pending', 'failed')
            ORDER BY o.created_at
            LIMIT $1
            "#,
            &[&limit],
        )
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| PendingMemory {
            outbox_id: row.get(0),
            scan_id: row.get(1),
            dataset_name: row.get(2),
            content: row.get(3),
        })
        .collect())
}

async fn send_to_cognee(settings: &Settings, client: &Client, memory: &PendingMemory) -> Result<()> {
    let url = cognee_url(settings, "remember");

    // Cognee 1.2.x expects the `data` field to be a multipart UploadFile list.
    // Sending `.text("data", ...)` makes FastAPI receive a string and returns:
    // `Expected UploadFile, received: <class 'str'>`.
    let file_name = format!(
        "osai-memory-outbox-{}-{}.md",
        memory.outbox_id,
        sanitize_file_component(&memory.scan_id)
    );
    // Redact before sending to Cognee. Raw local evidence remains in
    // PostgreSQL/RustFS, but long-term AI memory should not retain secrets.
    let redacted_content = redact_secret_like_text(&memory.content);
    let memory_file = multipart::Part::bytes(redacted_content.into_bytes())
        .file_name(file_name)
        .mime_str("text/markdown")
        .context("failed to build Cognee memory upload part")?;

    let form = multipart::Form::new()
        .part("data", memory_file)
        .text("datasetName", memory.dataset_name.clone())
        .text("run_in_background", settings.cognee_run_in_background.to_string())
        .text("chunks_per_batch", settings.cognee_chunks_per_batch.clone());

    let response = client
        .post(url)
        .multipart(form)
        .send()
        .await
        .context("failed to call Cognee remember endpoint")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_else(|_| "<failed to read response body>".to_string());
        anyhow::bail!("Cognee remember endpoint returned {status}: {body}");
    }

    Ok(())
}

fn sanitize_file_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn redact_secret_like_text(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if lower.contains("password=")
                || lower.contains("token=")
                || lower.contains("api_key")
                || lower.contains("authorization:")
                || lower.contains("bearer ")
                || lower.contains("private key")
                || lower.contains("secret_key")
                || lower.contains("access_key")
            {
                "[REDACTED secret-like line]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn mark_ingested(pg: &PgClient, outbox_id: i64) -> Result<()> {
    pg.execute(
        r#"
        UPDATE osai_cognee_outbox
        SET status = 'ingested',
            last_error = NULL,
            updated_at = now(),
            ingested_at = now()
        WHERE id = $1
        "#,
        &[&outbox_id],
    )
    .await?;
    Ok(())
}

async fn mark_failed(pg: &PgClient, outbox_id: i64, error: &str) -> Result<()> {
    pg.execute(
        r#"
        UPDATE osai_cognee_outbox
        SET status = 'failed',
            attempt_count = attempt_count + 1,
            last_error = $2,
            updated_at = now()
        WHERE id = $1
        "#,
        &[&outbox_id, &error],
    )
    .await?;
    Ok(())
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn cognee_url(settings: &Settings, endpoint: &str) -> String {
    format!(
        "{}{}/{}",
        settings.cognee_api_url,
        settings.cognee_api_prefix,
        endpoint.trim_start_matches('/')
    )
}

fn normalize_api_prefix(value: &str) -> String {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

fn env_optional(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
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
