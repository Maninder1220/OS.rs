// =============================================================================
// File: src/bin/osai-ask.rs
// Purpose:
//   Terminal Ask OSAI client using PostgreSQL latest facts, Cognee recall, and llama.cpp/Qwen.
//
// Where this fits in OSAI:
//   CLI equivalent of browser Ask OSAI for operators working over SSH.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Failures in Cognee recall should not block answering from local Postgres facts.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Client,
};
use serde_json::{json, Value};
use tokio_postgres::{Client as PgClient, NoTls};
use tracing::warn;

#[derive(Parser, Debug)]
#[command(name = "osai-ask")]
#[command(about = "Recall Cognee memory, combine it with latest OSAI facts, and ask local llama.cpp/Qwen")]
struct Args {
    /// Question to ask the local reasoning layer.
    #[arg(required = true)]
    question: Vec<String>,
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
    cognee_dataset: String,
    cognee_recall_timeout_seconds: u64,
    llm_endpoint: String,
    llm_api_key: String,
    llm_model: String,
    llm_max_tokens: u64,
    llm_timeout_seconds: u64,
}

#[derive(Debug)]
struct LatestScanContext {
    id: String,
    generated_at: String,
    hostname: String,
    highest_severity: String,
    finding_count: i32,
    object_store_key: Option<String>,
    snapshot: Value,
    findings: Vec<FindingContext>,
}

#[derive(Debug)]
struct FindingContext {
    severity: String,
    title: String,
    detail: Option<String>,
    recommendation: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string()))
        .compact()
        .init();

    load_env_files();
    let args = Args::parse();
    let question = args.question.join(" ");
    let settings = Settings::from_env();
    let client = build_http_client(&settings)?;
    let pg = connect_postgres(&settings).await?;

    let latest = load_latest_scan(&pg).await?;
    let cognee_context = recall_cognee(&settings, &client, &question).await.unwrap_or_else(|err| {
        warn!("Cognee recall failed: {err:#}");
        format!("Cognee recall failed: {err:#}")
    });
    let prompt = build_prompt(&question, latest.as_ref(), &cognee_context);
    let answer = ask_llama_cpp(&settings, &client, &prompt).await?;

    println!("{answer}");
    Ok(())
}

fn load_env_files() {
    let _ = dotenvy::from_filename(".env.storage");
    let _ = dotenvy::from_filename(".env.cognee");
    let _ = dotenvy::dotenv();
}

impl Settings {
    fn from_env() -> Self {
        let llm_model = env_or("OSAI_LLM_MODEL", &normalize_llm_model(&env_or("LLM_MODEL", "osai-llm")));
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
            cognee_dataset: env_or("COGNEE_DATASET", "osai-agent-memory"),
            cognee_recall_timeout_seconds: env_u64("OSAI_COGNEE_RECALL_TIMEOUT_SECONDS", 30),
            llm_endpoint: env_or("OSAI_LLM_ENDPOINT", &env_or("LLM_ENDPOINT", "http://127.0.0.1:8080/v1"))
                .trim_end_matches('/')
                .to_string(),
            llm_api_key: env_or("OSAI_LLM_API_KEY", &env_or("LLM_API_KEY", ".")),
            llm_max_tokens: env_u64("OSAI_LLM_MAX_TOKENS", 420),
            llm_timeout_seconds: env_u64("OSAI_LLM_TIMEOUT_SECONDS", 180),
            llm_model,
        }
    }
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
        .timeout(Duration::from_secs(180))
        .build()?)
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

async fn load_latest_scan(pg: &PgClient) -> Result<Option<LatestScanContext>> {
    let Some(row) = pg
        .query_opt(
            r#"
            SELECT id, generated_at::text, hostname, highest_severity,
                   finding_count, object_store_key, snapshot_json
            FROM osai_scan_history
            ORDER BY generated_at DESC
            LIMIT 1
            "#,
            &[],
        )
        .await?
    else {
        return Ok(None);
    };

    let id: String = row.get(0);
    let finding_rows = pg
        .query(
            r#"
            SELECT severity, title, detail, recommendation
            FROM osai_findings
            WHERE scan_id = $1
            ORDER BY created_at DESC
            LIMIT 20
            "#,
            &[&id],
        )
        .await?;

    Ok(Some(LatestScanContext {
        id,
        generated_at: row.get(1),
        hostname: row.get(2),
        highest_severity: row.get(3),
        finding_count: row.get(4),
        object_store_key: row.get(5),
        snapshot: row.get(6),
        findings: finding_rows
            .into_iter()
            .map(|finding| FindingContext {
                severity: finding.get(0),
                title: finding.get(1),
                detail: finding.get(2),
                recommendation: finding.get(3),
            })
            .collect(),
    }))
}

async fn recall_cognee(settings: &Settings, client: &Client, question: &str) -> Result<String> {
    let url = cognee_url(settings, "recall");
    let payload = json!({
        "query": question,
        "datasets": [settings.cognee_dataset],
        "search_type": "GRAPH_COMPLETION",
        "top_k": 5,
        "only_context": true,
        "verbose": false
    });

    let response: Value = client
        .post(url)
        .json(&payload)
        .timeout(Duration::from_secs(settings.cognee_recall_timeout_seconds.max(5)))
        .send()
        .await
        .context("failed to call Cognee recall endpoint")?
        .error_for_status()
        .context("Cognee recall endpoint returned an error")?
        .json()
        .await
        .context("failed to parse Cognee recall response")?;

    Ok(extract_context_text(&response))
}

async fn ask_llama_cpp(settings: &Settings, client: &Client, prompt: &str) -> Result<String> {
    let url = format!("{}/chat/completions", settings.llm_endpoint);
    let payload = json!({
        "model": settings.llm_model,
        "messages": [
            {
                "role": "system",
                "content": "You are OSAI, a local Linux and DevOps operations reasoning layer. Rust is the source of truth. Cognee is remembered operational context. Answer naturally and clearly for a human operator. Use only provided facts and recalled memory. Do not invent metrics, logs, paths, or service state. Do not execute repair actions. Include current status, seriousness, evidence, next safe checks, and what to do next. Do not output <think>."
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "temperature": 0.2,
        "max_tokens": settings.llm_max_tokens,
        "chat_template_kwargs": {
            "enable_thinking": false
        }
    });

    let response: Value = client
        .post(url)
        .bearer_auth(&settings.llm_api_key)
        .json(&payload)
        .timeout(Duration::from_secs(settings.llm_timeout_seconds.max(30)))
        .send()
        .await
        .context("failed to call llama.cpp chat completions endpoint")?
        .error_for_status()
        .context("llama.cpp chat completions endpoint returned an error")?
        .json()
        .await
        .context("failed to parse llama.cpp response")?;

    Ok(response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or_else(|| response.as_str().unwrap_or(""))
        .to_string())
}

fn build_prompt(question: &str, latest: Option<&LatestScanContext>, cognee_context: &str) -> String {
    let mut sections = vec![
        "User question:".to_string(),
        question.to_string(),
        String::new(),
        "Cognee recalled context:".to_string(),
        if cognee_context.trim().is_empty() {
            "No Cognee context returned.".to_string()
        } else {
            trim_to_chars(cognee_context, 1_500)
        },
    ];

    if let Some(scan) = latest {
        sections.extend([
            String::new(),
            "Latest PostgreSQL facts:".to_string(),
            format!("scan_id: {}", scan.id),
            format!("generated_at: {}", scan.generated_at),
            format!("hostname: {}", scan.hostname),
            format!("highest_severity: {}", scan.highest_severity),
            format!("finding_count: {}", scan.finding_count),
            format!(
                "raw_rustfs_object: {}",
                scan.object_store_key.as_deref().unwrap_or("none")
            ),
            "Findings:".to_string(),
        ]);

        if scan.findings.is_empty() {
            sections.push("- no findings stored".to_string());
        } else {
            sections.extend(scan.findings.iter().map(|finding| {
                format!(
                    "- [{}] {}. Detail: {} Recommendation: {}",
                    finding.severity,
                    finding.title,
                    finding.detail.as_deref().unwrap_or(""),
                    finding.recommendation.as_deref().unwrap_or("")
                )
            }));
        }
    } else {
        sections.extend([
            String::new(),
            "Latest PostgreSQL facts:".to_string(),
            "No scan data is stored yet.".to_string(),
        ]);
    }

    sections.join("\n")
}

fn extract_context_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(extract_context_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(map) => {
            for key in ["context", "answer", "text", "content", "source"] {
                if let Some(text) = map.get(key).and_then(Value::as_str) {
                    return text.to_string();
                }
            }
            value.to_string()
        }
        _ => value.to_string(),
    }
}

fn normalize_llm_model(model: &str) -> String {
    model.strip_prefix("openai/").unwrap_or(model).to_string()
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

fn trim_to_chars(value: &str, max_chars: usize) -> String {
    let mut text = value.trim().to_string();
    if text.len() <= max_chars {
        return text;
    }

    text.truncate(max_chars);
    text.push_str("\n...[trimmed]");
    text
}
