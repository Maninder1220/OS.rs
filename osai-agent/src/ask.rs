// =============================================================================
// File: src/ask.rs
// Purpose:
//   Browser Ask OSAI engine that combines Postgres facts, Cognee memory, and llama.cpp/Qwen answers.
//
// Where this fits in OSAI:
//   Called by /api/ask from the web dashboard.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Rust/PostgreSQL facts are authoritative. Cognee is memory. Qwen only formats and reasons over supplied context.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Client,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_postgres::{Client as PgClient, NoTls};
use tracing::warn;

use crate::{
    ask_plan::{plan_question, AskPlan},
    collector::Snapshot,
    fact_pack::{build_fact_pack, FactPack, FactPackSummary},
    intent::{analyze_question, build_deterministic_answer, QueryInsight},
    knowledge::{KnowledgeBase, KnowledgeMatch},
};

#[derive(Debug, Clone, Deserialize)]
pub struct AskRequest {
    pub question: String,
    #[serde(default)]
    pub use_ai: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AskResponse {
    pub answer: String,
    pub model: String,
    pub mode: String,
    pub ai_requested: bool,
    pub ai_used: bool,
    pub postgres_status: String,
    pub cognee_status: String,
    pub llama_status: String,
    pub inference_status: InferenceStatus,
    pub query_insights: Vec<QueryInsight>,
    pub ask_plan: AskPlan,
    pub fact_pack_summary: FactPackSummary,
    pub knowledge_matches: Vec<KnowledgeMatch>,
    pub latest_scan: Option<LatestScanContext>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InferenceStatus {
    pub ready: bool,
    pub endpoint: String,
    pub health_url: String,
    pub model: String,
    pub status: String,
    pub detail: String,
    pub recommended_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LatestScanContext {
    pub id: String,
    pub generated_at: String,
    pub hostname: String,
    pub highest_severity: String,
    pub finding_count: i32,
    pub object_store_key: Option<String>,
    pub snapshot_summary: Value,
    pub findings: Vec<FindingContext>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindingContext {
    pub severity: String,
    pub title: String,
    pub detail: Option<String>,
    pub recommendation: Option<String>,
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
    cognee_recall_with_ai_off: bool,
    cognee_recall_timeout_seconds: u64,
    llm_endpoint: String,
    llm_api_key: String,
    llm_model: String,
    llm_timeout_seconds: u64,
    llm_max_tokens: u64,
}

pub async fn ask_osai(
    request: AskRequest,
    knowledge: &KnowledgeBase,
    current_snapshot: &Snapshot,
) -> Result<AskResponse> {
    load_env_files();
    let settings = Settings::from_env();
    let client = build_http_client(&settings)?;
    let knowledge_matches = knowledge.search(&request.question, 6);
    let guidance = load_guidance(knowledge);
    let query_insights = analyze_question(&request.question, current_snapshot);
    let ask_plan = plan_question(&request.question, current_snapshot);
    // Rust always prepares a complete deterministic answer first. This keeps
    // Ask OSAI useful even when Cognee, PostgreSQL, or llama.cpp is offline.
    let deterministic_answer =
        build_deterministic_answer(&request.question, &query_insights, current_snapshot);
    let ai_requested = request.use_ai;
    // The model call is gated by a lightweight health check, so a loading model
    // returns a clear status instead of making the browser wait on a failing chat call.
    let inference_status = if ai_requested {
        check_inference_layer(&settings, &client).await
    } else {
        disabled_inference_status(&settings)
    };

    let mut latest_scan: Option<LatestScanContext> = None;
    let mut postgres_status = "skipped: ai off".to_string();
    let mut cognee_context = "Cognee recall skipped.".to_string();
    let mut cognee_status = "skipped".to_string();

    if ask_plan.use_cognee && (settings.cognee_recall_with_ai_off || ai_requested) {
        // Cognee recall is treated as extra memory context. It should enrich an
        // answer, not replace current scanner facts or hide local failures.
        let recall_query = build_recall_query(&ask_plan, current_snapshot);
        (cognee_context, cognee_status) =
            match recall_cognee(&settings, &client, &recall_query).await {
                Ok(context) if context.trim().is_empty() => {
                    ("No Cognee context returned.".to_string(), "empty".to_string())
                }
                Ok(context) => (context, "ok".to_string()),
                Err(err) => {
                    warn!("Cognee recall failed during Ask OSAI: {err:#}");
                    (
                        format!("Cognee recall failed: {err:#}"),
                        format!("failed: {err}"),
                    )
                }
            };
    } else if !ask_plan.use_cognee {
        cognee_status = "skipped: ask plan did not need memory recall".to_string();
        cognee_context = "Cognee recall skipped because Rust detected a simple live-status question.".to_string();
    }

    if ai_requested && inference_status.ready {
        // PostgreSQL is only queried for the heavier latest-scan context when
        // the AI layer is actually going to use it.
        (latest_scan, postgres_status) = match connect_postgres(&settings).await {
            Ok(pg) => match load_latest_scan(&pg).await {
                Ok(scan) => (scan, "ok".to_string()),
                Err(err) => {
                    warn!("failed to load latest PostgreSQL scan context: {err:#}");
                    (None, format!("failed: {err}"))
                }
            },
            Err(err) => {
                warn!("failed to connect to PostgreSQL for Ask OSAI: {err:#}");
                (None, format!("failed: {err}"))
            }
        };
    } else if ai_requested {
        postgres_status = "skipped: inference not ready".to_string();
    }

    let fact_pack = build_fact_pack(&ask_plan, current_snapshot, latest_scan.as_ref());
    let fact_pack_summary = fact_pack.summary();

    // The prompt is assembled from bounded, known pieces. AskPlan and FactPack
    // are the main prompt-budget controls: Rust decides what facts are relevant
    // before Cognee/Qwen see the question.
    let prompt = build_prompt(
        &ask_plan,
        &fact_pack,
        latest_scan.as_ref(),
        current_snapshot,
        &knowledge_matches,
        &guidance,
        &cognee_context,
        &query_insights,
        &inference_status,
    );
    let llm_max_tokens = settings.llm_max_tokens.min(ask_plan.llm_max_tokens.max(80));
    let (answer, llama_status, mode, ai_used) = if ai_requested && inference_status.ready {
        match ask_llama_cpp(&settings, &client, &prompt, llm_max_tokens).await {
            Ok(answer) if !answer.trim().is_empty() => (
                answer,
                "ok".to_string(),
                "rust insights refined by llama.cpp/qwen reasoning layer".to_string(),
                true,
            ),
            Ok(_) => (
                answer_with_cognee_recall(deterministic_answer, &cognee_context, &cognee_status),
                "empty".to_string(),
                "deterministic rust insights fallback".to_string(),
                false,
            ),
            Err(err) => {
                warn!("llama.cpp/Qwen failed during Ask OSAI, returning deterministic answer: {err:#}");
                (
                    answer_with_cognee_recall(deterministic_answer, &cognee_context, &cognee_status),
                    format!("failed: {err}"),
                    "deterministic rust insights fallback".to_string(),
                    false,
                )
            }
        }
    } else if ai_requested {
        (
            answer_with_cognee_recall(deterministic_answer, &cognee_context, &cognee_status),
            format!("not_ready: {}", inference_status.status),
            "deterministic rust insights fallback; inference layer not ready".to_string(),
            false,
        )
    } else {
        (
            answer_with_cognee_recall(deterministic_answer, &cognee_context, &cognee_status),
            "disabled_by_user".to_string(),
            "deterministic rust insights plus Cognee Cloud recall; AI refinement is off".to_string(),
            false,
        )
    };

    Ok(AskResponse {
        answer,
        model: settings.llm_model,
        mode,
        ai_requested,
        ai_used,
        postgres_status,
        cognee_status,
        llama_status,
        inference_status,
        query_insights,
        ask_plan,
        fact_pack_summary,
        knowledge_matches,
        latest_scan,
    })
}

fn load_env_files() {
    let _ = dotenvy::from_filename(".env.storage");
    let _ = dotenvy::from_filename(".env.cognee");
    let _ = dotenvy::dotenv();
}

impl Settings {
    fn from_env() -> Self {
        let llm_model = env_or(
            "OSAI_LLM_MODEL",
            &normalize_llm_model(&env_or("LLM_MODEL", "osai-llm")),
        );

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
            cognee_recall_with_ai_off: env_bool("OSAI_COGNEE_RECALL_WITH_AI_OFF", true),
            cognee_recall_timeout_seconds: env_u64("OSAI_COGNEE_RECALL_TIMEOUT_SECONDS", 30),
            llm_endpoint: env_or(
                "OSAI_LLM_ENDPOINT",
                &env_or("LLM_ENDPOINT", "http://127.0.0.1:8080/v1"),
            )
            .trim_end_matches('/')
            .to_string(),
            llm_api_key: env_or("OSAI_LLM_API_KEY", &env_or("LLM_API_KEY", "sk-no-key-required")),
            llm_timeout_seconds: env_u64("OSAI_LLM_TIMEOUT_SECONDS", 600),
            llm_max_tokens: env_u64("OSAI_LLM_MAX_TOKENS", 80),
            llm_model,
        }
    }
}

fn disabled_inference_status(settings: &Settings) -> InferenceStatus {
    let server_url = llama_server_url(&settings.llm_endpoint);
    InferenceStatus {
        ready: false,
        endpoint: settings.llm_endpoint.clone(),
        health_url: format!("{server_url}/health"),
        model: settings.llm_model.clone(),
        status: "disabled_by_user".to_string(),
        detail: "AI refinement is off. Ask OSAI is using deterministic Rust scanner logic only.".to_string(),
        recommended_checks: vec![format!("curl {server_url}/health")],
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

async fn check_inference_layer(settings: &Settings, client: &Client) -> InferenceStatus {
    let server_url = llama_server_url(&settings.llm_endpoint);
    let health_url = format!("{server_url}/health");
    let recommended_checks = vec![
        format!("curl {health_url}"),
        format!("curl {}/models", settings.llm_endpoint),
        "docker compose -f docker-compose.storage.yml ps".to_string(),
        "docker logs osai-llama --tail 100".to_string(),
        "ls -lh models/Qwen3-4B-Q4_K_M.gguf".to_string(),
    ];

    match client.get(&health_url).send().await {
        Ok(response) => {
            let status_code = response.status();
            let detail = response
                .text()
                .await
                .unwrap_or_else(|_| "health response body could not be read".to_string());
            let ready = status_code.is_success();
            let status = if ready {
                "ready"
            } else if status_code.as_u16() == 503 {
                "not_ready_or_loading"
            } else {
                "unhealthy"
            };

            InferenceStatus {
                ready,
                endpoint: settings.llm_endpoint.clone(),
                health_url,
                model: settings.llm_model.clone(),
                status: status.to_string(),
                detail,
                recommended_checks,
            }
        }
        Err(err) => InferenceStatus {
            ready: false,
            endpoint: settings.llm_endpoint.clone(),
            health_url,
            model: settings.llm_model.clone(),
            status: "unreachable".to_string(),
            detail: err.to_string(),
            recommended_checks,
        },
    }
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
    let snapshot: Value = row.get(6);
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
        snapshot_summary: json!({
            "host": snapshot.get("host").cloned().unwrap_or(Value::Null),
            "os": snapshot.get("os").cloned().unwrap_or(Value::Null),
            "memory": snapshot.get("memory").cloned().unwrap_or(Value::Null),
            "compute": snapshot.get("compute").cloned().unwrap_or(Value::Null),
            "storage": snapshot.get("storage").cloned().unwrap_or(Value::Null),
            "kubernetes": snapshot.get("kubernetes").cloned().unwrap_or(Value::Null),
            "gitlab": snapshot.get("gitlab").cloned().unwrap_or(Value::Null),
        }),
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

fn answer_with_cognee_recall(answer: String, cognee_context: &str, cognee_status: &str) -> String {
    if cognee_status != "ok" || cognee_context.trim().is_empty() {
        return answer;
    }

    [
        answer,
        String::new(),
        "## Recalled Cognee Memory".to_string(),
        trim_to_chars(cognee_context, 1_200),
    ]
    .join("\n")
}

async fn ask_llama_cpp(settings: &Settings, client: &Client, prompt: &str, max_tokens: u64) -> Result<String> {
    let url = format!("{}/chat/completions", settings.llm_endpoint);
    let payload = json!({
        "model": settings.llm_model,
        "messages": [
            {
                "role": "system",
                "content": "You are OSAI, a local Linux and DevOps operations reasoning layer. Rust is the source of truth. Cognee is remembered operational context. Your job is to turn those facts into clear human language. Answer naturally, calmly, and descriptively. Do not invent metrics, paths, services, logs, or command output. Do not execute repair actions. Prefer read-only diagnosis. Always explain: what we are looking at, how serious it is, what evidence supports it, what to check next, and the safest next command list. Do not output <think>."
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "temperature": 0.2,
        "max_tokens": max_tokens,
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
        .trim()
        .to_string())
}

fn build_prompt(
    ask_plan: &AskPlan,
    fact_pack: &FactPack,
    latest: Option<&LatestScanContext>,
    current_snapshot: &Snapshot,
    knowledge_matches: &[KnowledgeMatch],
    guidance: &str,
    cognee_context: &str,
    query_insights: &[QueryInsight],
    inference_status: &InferenceStatus,
) -> String {
    let knowledge_context = if knowledge_matches.is_empty() {
        "No matching local Markdown knowledge files.".to_string()
    } else {
        trim_to_chars(&knowledge_matches
            .iter()
            .take(2)
            .map(|item| format!("## {}\n{}", item.file, item.excerpt))
            .collect::<Vec<_>>()
            .join("\n\n"), 1_200)
    };

    let latest_context = match latest {
        Some(scan) => {
            let findings = if scan.findings.is_empty() {
                "no stored findings".to_string()
            } else {
                scan.findings
                    .iter()
                    .take(5)
                    .map(|finding| {
                        format!(
                            "- [{}] {} | {}",
                            finding.severity,
                            finding.title,
                            finding.recommendation.as_deref().unwrap_or("review manually")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            format!(
                "scan_id={}\ngenerated_at={}\nhostname={}\nhighest_severity={}\nfinding_count={}\nraw_object={}\nfindings:\n{}",
                scan.id,
                scan.generated_at,
                scan.hostname,
                scan.highest_severity,
                scan.finding_count,
                scan.object_store_key.as_deref().unwrap_or("none"),
                findings
            )
        }
        None => format!(
            "No PostgreSQL scan row was available. Current in-memory snapshot summary:\nhost={} os={} findings={}",
            current_snapshot.host.hostname,
            current_snapshot.os.long_version,
            current_snapshot.findings.len()
        ),
    };
    let deterministic_context = if query_insights.is_empty() {
        "No deterministic Rust insight matched this question.".to_string()
    } else {
        trim_to_chars(
            &serde_json::to_string_pretty(
                &query_insights.iter().take(3).collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "Deterministic insight context could not be serialized.".to_string()),
            2_000,
        )
    };
    let inference_context = serde_json::to_string_pretty(inference_status)
        .unwrap_or_else(|_| "Inference status could not be serialized.".to_string());
    let ask_plan_context = serde_json::to_string_pretty(ask_plan)
        .unwrap_or_else(|_| "AskPlan could not be serialized.".to_string());
    let fact_pack_context = serde_json::to_string_pretty(fact_pack)
        .unwrap_or_else(|_| "FactPack could not be serialized.".to_string());

    [
        "# User Question".to_string(),
        ask_plan.original_question.clone(),
        String::new(),
        "# Rust AskPlan".to_string(),
        ask_plan_context,
        String::new(),
        "# Focused FactPack".to_string(),
        fact_pack_context,
        String::new(),
        "# Inference And Reasoning Layer Status".to_string(),
        inference_context,
        String::new(),
        "# Deterministic Rust Insights".to_string(),
        deterministic_context,
        String::new(),
        "# Operator Guidance".to_string(),
        trim_to_chars(guidance, 1_000),
        String::new(),
        "# Latest Facts".to_string(),
        latest_context,
        String::new(),
        "# Recalled Cognee Memory".to_string(),
        trim_to_chars(cognee_context, 1_500),
        String::new(),
        "# Local Markdown Knowledge Matches".to_string(),
        knowledge_context,
        String::new(),
        "# Answer Rules".to_string(),
        "- Start conversationally with the direct answer to the user's question.".to_string(),
        "- Use the Focused FactPack first. Treat broad Latest Facts as fallback context only.".to_string(),
        "- Stay focused on this machine, this project, and operational troubleshooting.".to_string(),
        "- Use sections: Current status, Why it matters, Evidence, Next safe checks, What I would do next.".to_string(),
        "- Prefer read-only checks first.".to_string(),
        "- Keep it understandable for a human operator. Explain what the metric or signal means before giving commands.".to_string(),
        "- Mention when Cognee or PostgreSQL context is unavailable.".to_string(),
        "- Do not claim you changed the system unless an approved action result is provided.".to_string(),
        "- When deterministic insights include manual_checks, list the relevant commands under safe manual checks.".to_string(),
        "- Explain seriousness in plain operator language: stable, needs attention, high risk, or critical.".to_string(),
    ]
    .join("\n")
}

fn build_recall_query(ask_plan: &AskPlan, current_snapshot: &Snapshot) -> String {
    let intents = ask_plan
        .intents
        .iter()
        .map(|intent| format!("{intent:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "host {} previous incidents repeated patterns resolved issues operational memory for intents: {} question: {}",
        current_snapshot.host.hostname, intents, ask_plan.original_question
    )
}

fn load_guidance(knowledge: &KnowledgeBase) -> String {
    let mut sections = Vec::new();
    for name in [
        "00_agent_identity.md",
        "08_response_format.md",
        "09_inference_reasoning_guidance.md",
    ] {
        if let Some(content) = knowledge.get(name) {
            sections.push(format!("## {name}\n{content}"));
        }
    }

    if sections.is_empty() {
        "No dedicated guidance file was loaded. Use concise operational answers.".to_string()
    } else {
        sections.join("\n\n")
    }
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

fn llama_server_url(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .strip_suffix("/v1")
        .unwrap_or_else(|| endpoint.trim_end_matches('/'))
        .trim_end_matches('/')
        .to_string()
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
