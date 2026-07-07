// =============================================================================
// File: src/main.rs
// Purpose:
//   Main OSAI web server: scanner state, dashboard assets, REST APIs, Ask OSAI, reasoning, and guarded actions.
//
// Where this fits in OSAI:
//   This is the primary long-running Rust process that operators open in the browser.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Keep API auth, action approval, and scanner state handling conservative because this process can expose host data.
// =============================================================================
// -----------------------------------------------------------------------------
// Module wiring
// -----------------------------------------------------------------------------

mod actions;
mod ask;
mod ask_plan;
mod collector;
mod cognee_lifecycle;
mod fact_pack;
mod history;
mod intent;
mod knowledge;
mod plugins;
mod reasoning;
mod rules;

// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use actions::{ActionRequest, ActionStore};
use ask::{ask_osai, AskRequest, AskResponse};
use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, Response, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use cognee_lifecycle::{
    CogneeLifecycleClient, CogneeLifecycleStatus, ForgetMemoryRequest, ForgetMemoryResponse,
    MemoryFeedbackRequest, MemoryFeedbackResponse,
};
use history::{HistoryRecord, HistoryStore, HistorySummary};
use include_dir::{include_dir, Dir};
use reasoning::{reason_about, ReasonRequest, ReasonResponse};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use collector::{collect_snapshot, Snapshot};
use knowledge::KnowledgeBase;

static WEB_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web");

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

#[derive(Parser, Debug)]
#[command(name = "osai-agent")]
#[command(about = "Rust-first local OS AI agent")]
struct Args {
    /// Address where the local dashboard/API should listen.
    #[arg(long, default_value = "127.0.0.1:8000")]
    bind: SocketAddr,

    /// Directory containing operator knowledge Markdown files.
    #[arg(long, default_value = "knowledge")]
    knowledge_dir: PathBuf,

    /// Directory used for persistent scan history and action audit logs.
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,

    /// Background scan interval in seconds.
    #[arg(long, default_value_t = 30)]
    scan_interval_seconds: u64,

    /// API token required by the dashboard/API. Also read from OSAI_AGENT_TOKEN.
    #[arg(long)]
    api_token: Option<String>,

    /// Allow binding outside localhost without API token. Not recommended.
    #[arg(long, default_value_t = false)]
    allow_insecure_public_dashboard: bool,
}

#[derive(Clone)]
struct AppState {
    snapshot: Arc<RwLock<Snapshot>>,
    knowledge: Arc<KnowledgeBase>,
    history: Arc<HistoryStore>,
    actions: Arc<ActionStore>,
    api_token: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    mode: &'static str,
    auth_required: bool,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<usize>,
}

#[derive(Serialize)]
struct PluginResponse {
    kubernetes: collector::models::KubernetesHint,
    gitlab: collector::models::GitlabHint,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "osai_agent=info,tower_http=info,axum=info".to_string()),
        )
        .compact()
        .init();

    let args = Args::parse();
    let api_token = args
        .api_token
        .or_else(|| std::env::var("OSAI_AGENT_TOKEN").ok())
        .filter(|token| !token.trim().is_empty());

    if !args.bind.ip().is_loopback()
        && api_token.is_none()
        && !args.allow_insecure_public_dashboard
    {
        // Binding the dashboard to a public interface without a token would
        // expose host facts and action endpoints, so fail closed by default.
        anyhow::bail!(
            "refusing to bind {bind} without auth. Set --api-token, OSAI_AGENT_TOKEN, or explicitly pass --allow-insecure-public-dashboard",
            bind = args.bind
        );
    }

    let knowledge = KnowledgeBase::load(&args.knowledge_dir)?;
    let history = Arc::new(HistoryStore::new(&args.data_dir)?);
    let actions = Arc::new(ActionStore::new(&args.data_dir)?);

    // Take one scan before serving traffic so /api/snapshot and the dashboard
    // have real host data immediately after startup.
    let first_snapshot = collect_snapshot().await;
    if let Err(err) = history.append_snapshot(&first_snapshot) {
        warn!("failed to persist first scan: {err}");
    }

    let state = AppState {
        snapshot: Arc::new(RwLock::new(first_snapshot)),
        knowledge: Arc::new(knowledge),
        history,
        actions,
        api_token,
    };

    let refresh_state = state.clone();
    let scan_interval = args.scan_interval_seconds.max(5);

    // Keep scan history fresh in the background. The API always reads the most
    // recent in-memory snapshot, while HistoryStore appends exact JSONL records.
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(scan_interval));
        loop {
            ticker.tick().await;
            let next_snapshot = collect_snapshot().await;
            if let Err(err) = refresh_state.history.append_snapshot(&next_snapshot) {
                warn!("failed to persist background scan: {err}");
            }
            let mut guard = refresh_state.snapshot.write().await;
            *guard = next_snapshot;
        }
    });

    // API routes are split by responsibility: facts, history, knowledge,
    // reasoning, Ask OSAI, plugins, and guarded actions. static_file serves the
    // embedded UI when the request is not an API route.
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/snapshot", get(snapshot))
        .route("/api/scan", post(scan_now))
        .route("/api/history", get(history_list))
        .route("/api/history/{id}", get(history_get))
        .route("/api/knowledge", get(list_knowledge))
        .route("/api/knowledge/{name}", get(read_knowledge))
        .route("/api/reason", post(reason))
        .route("/api/ask", post(ask))
        .route("/api/cognee/lifecycle", get(cognee_lifecycle_status))
        .route("/api/cognee/feedback", post(cognee_feedback))
        .route("/api/cognee/forget", post(cognee_forget))
        .route("/api/plugins", get(plugins))
        .route("/api/actions", get(list_actions))
        .route("/api/actions/propose", post(propose_action))
        .route("/api/actions/{id}/approve", post(approve_action))
        .route("/api/actions/{id}/run", post(run_action))
        .fallback(static_file)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    info!("OSAI Agent listening on http://{}", args.bind);
    info!("Mode: scanner + history + rules + guarded actions");
    info!("Knowledge directory: {}", args.knowledge_dir.display());
    info!("Data directory: {}", args.data_dir.display());

    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "osai-agent",
        mode: "guarded",
        auth_required: state.api_token.is_some(),
    })
}

async fn snapshot(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Snapshot> {
    verify_api_auth(&headers, &state)?;
    let guard = state.snapshot.read().await;
    Ok(Json(guard.clone()))
}

async fn scan_now(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Snapshot> {
    verify_api_auth(&headers, &state)?;
    let next = collect_snapshot().await;
    if let Err(err) = state.history.append_snapshot(&next) {
        warn!("failed to persist manual scan: {err}");
    }
    {
        let mut guard = state.snapshot.write().await;
        *guard = next.clone();
    }
    Ok(Json(next))
}

async fn history_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Vec<HistorySummary>> {
    verify_api_auth(&headers, &state)?;
    let limit = query.limit.unwrap_or(25).clamp(1, 200);
    state
        .history
        .list_recent(limit)
        .map(Json)
        .map_err(internal_error)
}

async fn history_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<HistoryRecord> {
    verify_api_auth(&headers, &state)?;
    match state.history.get(&id).map_err(internal_error)? {
        Some(record) => Ok(Json(record)),
        None => Err(not_found("history record not found")),
    }
}

async fn list_knowledge(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Vec<String>> {
    verify_api_auth(&headers, &state)?;
    Ok(Json(state.knowledge.list()))
}

async fn read_knowledge(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
) -> impl IntoResponse {
    if let Err(err) = verify_api_auth(&headers, &state) {
        return err.into_response();
    }

    match state.knowledge.get(&name) {
        Some(content) => (StatusCode::OK, content).into_response(),
        None => not_found("knowledge file not found").into_response(),
    }
}

async fn reason(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReasonRequest>,
) -> ApiResult<ReasonResponse> {
    verify_api_auth(&headers, &state)?;
    let snapshot = state.snapshot.read().await.clone();
    Ok(Json(reason_about(
        &request.question,
        &snapshot,
        state.knowledge.as_ref(),
    )))
}

async fn ask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AskRequest>,
) -> ApiResult<AskResponse> {
    verify_api_auth(&headers, &state)?;
    let snapshot = state.snapshot.read().await.clone();
    ask_osai(request, state.knowledge.as_ref(), &snapshot)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn cognee_lifecycle_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<CogneeLifecycleStatus> {
    verify_api_auth(&headers, &state)?;
    let client = CogneeLifecycleClient::from_env().map_err(internal_error)?;
    Ok(Json(client.health_check().await))
}

async fn cognee_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<MemoryFeedbackRequest>,
) -> ApiResult<MemoryFeedbackResponse> {
    verify_api_auth(&headers, &state)?;
    let client = CogneeLifecycleClient::from_env().map_err(internal_error)?;
    Ok(Json(client.remember_feedback(request).await))
}

async fn cognee_forget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ForgetMemoryRequest>,
) -> ApiResult<ForgetMemoryResponse> {
    verify_api_auth(&headers, &state)?;
    let client = CogneeLifecycleClient::from_env().map_err(internal_error)?;
    Ok(Json(client.forget_memory(request).await))
}

async fn plugins(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<PluginResponse> {
    verify_api_auth(&headers, &state)?;
    let snapshot = state.snapshot.read().await;
    Ok(Json(PluginResponse {
        kubernetes: snapshot.kubernetes.clone(),
        gitlab: snapshot.gitlab.clone(),
    }))
}

async fn list_actions(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Vec<actions::ActionRecord>> {
    verify_api_auth(&headers, &state)?;
    Ok(Json(state.actions.list()))
}

async fn propose_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ActionRequest>,
) -> ApiResult<actions::ActionRecord> {
    verify_api_auth(&headers, &state)?;
    state.actions.propose(request).map(Json).map_err(internal_error)
}

async fn approve_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<actions::ActionRecord> {
    verify_api_auth(&headers, &state)?;
    match state.actions.approve(&id).map_err(internal_error)? {
        Some(record) => Ok(Json(record)),
        None => Err(not_found("action not found")),
    }
}

async fn run_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<actions::ActionRecord> {
    verify_api_auth(&headers, &state)?;
    match state.actions.run(&id).await.map_err(internal_error)? {
        Some(record) => Ok(Json(record)),
        None => Err(not_found("action not found")),
    }
}

async fn static_file(uri: Uri) -> Response<Body> {
    let mut path = uri.path().trim_start_matches('/').to_string();

    if path.is_empty() {
        path = "index.html".to_string();
    }

    if path.contains("..") {
        return status_response(StatusCode::BAD_REQUEST, "invalid path");
    }

    match WEB_DIR.get_file(&path) {
        Some(file) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let mut response = Response::new(Body::from(file.contents().to_vec()));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime.as_ref())
                    .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
            );
            response
        }
        None => {
            error!("static asset not found: {}", path);
            status_response(StatusCode::NOT_FOUND, "not found")
        }
    }
}

fn verify_api_auth(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let Some(expected) = state.api_token.as_deref() else {
        return Ok(());
    };

    let token_header = headers
        .get("x-osai-token")
        .and_then(|value| value.to_str().ok());
    let bearer_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if token_header == Some(expected) || bearer_header == Some(expected) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                error: "missing or invalid OSAI API token".to_string(),
            }),
        ))
    }
}

fn internal_error(err: anyhow::Error) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error: err.to_string(),
        }),
    )
}

fn not_found(message: &str) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: message.to_string(),
        }),
    )
}

fn status_response(status: StatusCode, body: &'static str) -> Response<Body> {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
}
