// =============================================================================
// File: src/cognee_lifecycle.rs
// Purpose:
//   Centralizes Cognee remember, recall, improve, forget, health, feedback, and redaction behavior.
//
// Where this fits in OSAI:
//   Makes the project visibly Cognee-native instead of scattering memory lifecycle calls across binaries.
//
// Topics to know before editing:
//   Cognee REST endpoints, reqwest JSON/multipart calls, secret redaction, and OSAI memory datasets.
//
// Important operational notes:
//   Never send raw secret-like strings to long-term memory. Redact before remember/improve operations.
// =============================================================================

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    multipart, Client,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct CogneeLifecycleSettings {
    pub api_url: String,
    pub api_prefix: String,
    pub api_key: Option<String>,
    pub tenant_id: Option<String>,
    pub user_id: Option<String>,
    pub send_identity_headers: bool,
    pub send_bearer_auth: bool,
    pub dataset: String,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CogneeLifecycleStatus {
    pub api_url: String,
    pub dataset: String,
    pub remember: String,
    pub recall: String,
    pub improve: String,
    pub forget: String,
    pub health: String,
    pub last_detail: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryFeedbackRequest {
    pub question: String,
    pub answer: String,
    pub feedback: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub dataset_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryFeedbackResponse {
    pub remembered: bool,
    pub improved: bool,
    pub dataset: String,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForgetMemoryRequest {
    #[serde(default)]
    pub dataset_name: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForgetMemoryResponse {
    pub forgotten: bool,
    pub dataset: String,
    pub detail: String,
}

pub struct CogneeLifecycleClient {
    settings: CogneeLifecycleSettings,
    client: Client,
}

impl CogneeLifecycleSettings {
    pub fn from_env() -> Self {
        load_env_files();
        Self {
            api_url: env_or("COGNEE_API_URL", "http://127.0.0.1:8001")
                .trim_end_matches('/')
                .to_string(),
            api_prefix: normalize_api_prefix(&env_or("COGNEE_API_PREFIX", "/api/v1")),
            api_key: env_optional("COGNEE_API_KEY"),
            tenant_id: env_optional("COGNEE_TENANT_ID"),
            user_id: env_optional("COGNEE_USER_ID"),
            send_identity_headers: env_bool("OSAI_COGNEE_SEND_IDENTITY_HEADERS", false),
            send_bearer_auth: env_bool("OSAI_COGNEE_SEND_BEARER_AUTH", false),
            dataset: env_or("COGNEE_DATASET", "osai-agent-memory"),
            timeout_seconds: env_u64("OSAI_COGNEE_HTTP_TIMEOUT_SECONDS", 120),
        }
    }
}

impl CogneeLifecycleClient {
    pub fn from_env() -> Result<Self> {
        let settings = CogneeLifecycleSettings::from_env();
        let client = build_http_client(&settings)?;
        Ok(Self { settings, client })
    }

    pub async fn health_check(&self) -> CogneeLifecycleStatus {
        let docs_url = format!("{}/docs", self.settings.api_url);
        let (health, detail) = match self.client.get(&docs_url).send().await {
            Ok(response) if response.status().is_success() => ("ok".to_string(), "Cognee docs endpoint is reachable.".to_string()),
            Ok(response) => ("degraded".to_string(), format!("Cognee docs endpoint returned {}", response.status())),
            Err(err) => ("unreachable".to_string(), err.to_string()),
        };

        CogneeLifecycleStatus {
            api_url: self.settings.api_url.clone(),
            dataset: self.settings.dataset.clone(),
            remember: "available via /remember multipart upload".to_string(),
            recall: "available via /recall query".to_string(),
            improve: "best-effort via /improve when endpoint is present".to_string(),
            forget: "available via /forget when confirmed by operator".to_string(),
            health,
            last_detail: detail,
        }
    }

    pub async fn recall_memory(&self, query: &str) -> Result<String> {
        let response: Value = self.client
            .post(self.url("recall"))
            .json(&json!({
                "query": query,
                "datasets": [&self.settings.dataset],
                "search_type": "GRAPH_COMPLETION",
                "top_k": 5,
                "only_context": true,
                "verbose": false
            }))
            .timeout(Duration::from_secs(self.settings.timeout_seconds.max(5)))
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

    pub async fn remember_memory(&self, dataset: &str, file_name: &str, content: &str) -> Result<()> {
        let memory_file = multipart::Part::bytes(redact_secret_like_text(content).into_bytes())
            .file_name(file_name.to_string())
            .mime_str("text/markdown")
            .context("failed to build Cognee memory upload part")?;

        let form = multipart::Form::new()
            .part("data", memory_file)
            .text("datasetName", dataset.to_string())
            .text("run_in_background", "true")
            .text("chunks_per_batch", "4");

        let response = self.client
            .post(self.url("remember"))
            .multipart(form)
            .timeout(Duration::from_secs(self.settings.timeout_seconds.max(10)))
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

    pub async fn improve_memory(&self, dataset: &str, note: &str) -> Result<()> {
        let response = self.client
            .post(self.url("improve"))
            .json(&json!({
                "datasetName": dataset,
                "data": redact_secret_like_text(note),
            }))
            .timeout(Duration::from_secs(self.settings.timeout_seconds.max(10)))
            .send()
            .await
            .context("failed to call Cognee improve endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_else(|_| "<failed to read response body>".to_string());
            anyhow::bail!("Cognee improve endpoint returned {status}: {body}");
        }
        Ok(())
    }

    pub async fn remember_feedback(&self, request: MemoryFeedbackRequest) -> MemoryFeedbackResponse {
        let dataset = request.dataset_name.unwrap_or_else(|| self.settings.dataset.clone());
        let content = format!(
            "# OSAI Answer Feedback\n\nQuestion: {}\n\nFeedback: {}\n\nResolved: {}\n\nNote: {}\n\nAnswer:\n{}\n",
            request.question,
            request.feedback,
            request.resolved,
            request.note.as_deref().unwrap_or("none"),
            request.answer,
        );

        let remember_result = self
            .remember_memory(&dataset, "osai-answer-feedback.md", &content)
            .await;
        let improve_result = self.improve_memory(&dataset, &content).await;

        MemoryFeedbackResponse {
            remembered: remember_result.is_ok(),
            improved: improve_result.is_ok(),
            dataset,
            detail: format!(
                "remember={}; improve={}",
                result_label(&remember_result),
                result_label(&improve_result)
            ),
        }
    }

    pub async fn forget_memory(&self, request: ForgetMemoryRequest) -> ForgetMemoryResponse {
        let dataset = request.dataset_name.unwrap_or_else(|| self.settings.dataset.clone());
        if !request.confirm {
            return ForgetMemoryResponse {
                forgotten: false,
                dataset,
                detail: "refused: confirm=true is required before forgetting memory".to_string(),
            };
        }

        let response = self.client
            .post(self.url("forget"))
            .json(&json!({
                "dataset": &dataset,
                "datasetName": &dataset,
                "reason": request.reason.unwrap_or_else(|| "operator requested forget".to_string()),
            }))
            .timeout(Duration::from_secs(self.settings.timeout_seconds.max(10)))
            .send()
            .await;

        match response {
            Ok(response) if response.status().is_success() => ForgetMemoryResponse {
                forgotten: true,
                dataset,
                detail: "Cognee forget endpoint accepted the request.".to_string(),
            },
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_else(|_| "<failed to read response body>".to_string());
                ForgetMemoryResponse {
                    forgotten: false,
                    dataset,
                    detail: format!("Cognee forget endpoint returned {status}: {body}"),
                }
            }
            Err(err) => ForgetMemoryResponse {
                forgotten: false,
                dataset,
                detail: err.to_string(),
            },
        }
    }

    fn url(&self, endpoint: &str) -> String {
        format!(
            "{}{}/{}",
            self.settings.api_url,
            self.settings.api_prefix,
            endpoint.trim_start_matches('/')
        )
    }
}

pub fn redact_secret_like_text(input: &str) -> String {
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

fn build_http_client(settings: &CogneeLifecycleSettings) -> Result<Client> {
    let mut headers = HeaderMap::new();
    if let Some(api_key) = settings.api_key.as_deref() {
        headers.insert("x-api-key", HeaderValue::from_str(api_key)?);
        if settings.send_bearer_auth {
            headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {api_key}"))?);
        }
    }
    if settings.send_identity_headers {
        if let Some(tenant_id) = settings.tenant_id.as_deref() {
            headers.insert("x-cognee-tenant-id", HeaderValue::from_str(tenant_id)?);
        }
        if let Some(user_id) = settings.user_id.as_deref() {
            headers.insert("x-cognee-user-id", HeaderValue::from_str(user_id)?);
        }
    }
    Ok(Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(settings.timeout_seconds.max(10)))
        .build()?)
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

fn result_label<T>(result: &Result<T>) -> String {
    match result {
        Ok(_) => "ok".to_string(),
        Err(err) => format!("failed: {err}"),
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
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

fn normalize_api_prefix(value: &str) -> String {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

fn load_env_files() {
    let _ = dotenvy::from_filename(".env.storage");
    let _ = dotenvy::from_filename(".env.cognee");
    let _ = dotenvy::dotenv();
}
