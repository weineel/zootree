use super::error::{self, ApiError, PROTOCOL_HEADER};
use super::{api, Activity, CAPABILITIES, MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION};
use crate::config::ConfigManager;
use anyhow::Result;
use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Clone)]
pub(super) struct AppState {
    pub token: String,
    pub base_dir: std::path::PathBuf,
    pub pid: u32,
    pub started_at: String,
    pub activity: Arc<Activity>,
    pub shutdown: Arc<Notify>,
}

fn router(state: AppState) -> Router {
    // Authentication is its own layer so other listeners (later: loopback TCP)
    // can swap it without touching handlers.
    Router::new()
        .route("/status", get(status))
        .route("/shutdown", post(shutdown))
        .route("/workspaces", get(workspaces))
        .route("/workspaces/{name}", get(workspace))
        .route("/repos", get(repos))
        .route("/agents", get(agents))
        .fallback(not_found)
        .layer(middleware::from_fn(negotiate_version))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            track_activity,
        ))
        .with_state(state)
}

async fn track_activity(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let _guard = state.activity.begin_request();
    next.run(request).await
}

async fn authenticate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if presented != Some(state.token.as_str()) {
        return ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: error::UNAUTHORIZED,
            message: "missing or invalid token".to_string(),
        }
        .into_response();
    }
    next.run(request).await
}

/// Rejects clients that declare an incompatible protocol version. The header
/// is optional; `/status` is exempt so a client can always learn the Daemon's
/// versions to explain the mismatch. Runs after authentication.
async fn negotiate_version(request: Request, next: Next) -> Response {
    if request.uri().path() != "/status" {
        if let Some(raw) = request.headers().get(PROTOCOL_HEADER) {
            let declared = raw.to_str().ok().and_then(|v| v.trim().parse::<u32>().ok());
            let Some(declared) = declared else {
                return ApiError::bad_request(format!(
                    "{PROTOCOL_HEADER} must be a non-negative integer"
                ))
                .into_response();
            };
            if declared < MIN_COMPATIBLE_PROTOCOL_VERSION {
                return ApiError::new(
                    StatusCode::UPGRADE_REQUIRED,
                    error::CLIENT_TOO_OLD,
                    format!(
                        "client protocol version {declared} is older than the minimum {MIN_COMPATIBLE_PROTOCOL_VERSION} this daemon supports; please upgrade the client"
                    ),
                )
                .into_response();
            }
            if declared > PROTOCOL_VERSION {
                return ApiError::new(
                    StatusCode::UPGRADE_REQUIRED,
                    error::CLIENT_TOO_NEW,
                    format!(
                        "client protocol version {declared} is newer than this daemon's {PROTOCOL_VERSION}; please upgrade the daemon (zootree daemon stop, then start it again)"
                    ),
                )
                .into_response();
            }
        }
    }
    next.run(request).await
}

/// Runs synchronous core work off the async runtime.
async fn blocking<F>(state: &AppState, work: F) -> Result<Json<serde_json::Value>, ApiError>
where
    F: FnOnce(ConfigManager) -> Result<serde_json::Value, ApiError> + Send + 'static,
{
    let mgr = ConfigManager::with_base_dir(state.base_dir.clone());
    tokio::task::spawn_blocking(move || work(mgr))
        .await
        .map_err(|e| ApiError::internal(format!("read task failed: {e}")))?
        .map(Json)
}

async fn workspaces(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ApiError> {
    let filter = api::parse_status_filter(request.uri().query())?;
    blocking(&state, move |mgr| api::list_workspaces(&mgr, &filter)).await
}

async fn workspace(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(&state, move |mgr| api::workspace_detail(&mgr, &name)).await
}

async fn repos(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(&state, |mgr| api::list_repos(&mgr)).await
}

async fn agents(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(&state, |mgr| api::agent_catalog(&mgr)).await
}

async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "protocol_version": PROTOCOL_VERSION,
        "min_compatible_protocol_version": MIN_COMPATIBLE_PROTOCOL_VERSION,
        "capabilities": CAPABILITIES,
        "pid": state.pid,
        "started_at": state.started_at,
    }))
}

async fn shutdown(State(state): State<AppState>) -> Json<serde_json::Value> {
    state.shutdown.notify_one();
    Json(json!({ "stopping": true }))
}

async fn not_found() -> ApiError {
    ApiError::not_found("no such endpoint")
}

/// Runs the server until shutdown is requested, a signal arrives, or the idle
/// rule fires. Blocks the calling thread.
pub(super) fn run(
    listener: std::os::unix::net::UnixListener,
    state: AppState,
    idle_timeout: Duration,
    shutdown: Arc<Notify>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::UnixListener::from_std(listener)?;
        let activity = state.activity.clone();
        let app = router(state);
        let stop = shutdown.clone();
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = stop.notified() => {}
                    _ = wait_for_signal() => {}
                    _ = wait_until_idle(activity, idle_timeout) => {
                        tracing::info!("daemon idle; exiting");
                    }
                }
            })
            .await?;
        Ok(())
    })
}

async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut term), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

async fn wait_until_idle(activity: Arc<Activity>, timeout: Duration) {
    let tick = (timeout / 4).clamp(Duration::from_millis(20), Duration::from_secs(5));
    loop {
        tokio::time::sleep(tick).await;
        if activity.idle_for().is_some_and(|idle| idle >= timeout) {
            return;
        }
    }
}
