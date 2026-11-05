use super::auth::{self, Authenticator};
use super::error::{self, ApiError, PROTOCOL_HEADER};
use super::events::{self, EventHub};
use super::operations::{self, OperationRegistry, RunnerFactory};
use super::transport::Listener;
use super::{
    api, mutations, Activity, CAPABILITIES, MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION,
};
use crate::config::ConfigManager;
use crate::service::RequestContext;
use anyhow::Result;
use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

pub(super) type Accepted = Result<(StatusCode, Json<serde_json::Value>), ApiError>;

#[derive(Clone)]
pub(super) struct AppState {
    pub auth: Arc<dyn Authenticator>,
    pub base_dir: std::path::PathBuf,
    pub pid: u32,
    pub started_at: String,
    pub activity: Arc<Activity>,
    pub shutdown: Arc<Notify>,
    pub operations: Arc<OperationRegistry>,
    pub events: Arc<EventHub>,
    pub runner: RunnerFactory,
}

fn router(state: AppState) -> Router {
    // Authentication is its own layer (`auth`), so another transport can bring
    // a different Authenticator without touching handlers. Reserved seam: a
    // static-asset mount for a future Web UI would be attached to this router
    // (as a fallback service, behind its own auth decision); not implemented.
    Router::new()
        .route("/status", get(status))
        .route("/shutdown", post(shutdown))
        .route("/workspaces", get(workspaces))
        .route("/workspaces/{name}", get(workspace))
        .route("/repos", get(repos))
        .route("/agents", get(agents))
        .route("/workspaces/{name}/done", post(done))
        .route("/workspaces/{name}/cancel", post(cancel))
        .route("/operations/{id}", get(operation))
        .route("/events", get(event_stream))
        .merge(mutations::routes())
        .route(
            "/operations/{id}/cancel",
            post(super::cancel::cancel_operation),
        )
        .fallback(not_found)
        .layer(middleware::from_fn(negotiate_version))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
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

/// Rejects clients that declare no protocol version or an incompatible one.
/// The header is required everywhere except `/status`, which stays exempt so
/// a client can always learn the Daemon's versions to explain the mismatch.
/// Runs after authentication.
async fn negotiate_version(request: Request, next: Next) -> Response {
    if request.uri().path() != "/status" {
        let Some(raw) = request.headers().get(PROTOCOL_HEADER) else {
            return ApiError::new(
                StatusCode::UPGRADE_REQUIRED,
                error::PROTOCOL_VERSION_REQUIRED,
                format!(
                    "{PROTOCOL_HEADER} is required (this daemon speaks protocol {PROTOCOL_VERSION}, minimum compatible {MIN_COMPATIBLE_PROTOCOL_VERSION}); please upgrade the client"
                ),
            )
            .into_response();
        };
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

/// Accepts a state-changing request on an existing Workspace: checks that it
/// exists, then hands the job to [`spawn`].
async fn submit(
    state: &AppState,
    kind: &'static str,
    name: String,
    job: operations::Job,
) -> Accepted {
    let mgr = ConfigManager::with_base_dir(state.base_dir.clone());
    let check_name = name.clone();
    tokio::task::spawn_blocking(move || api::require_workspace(&mgr, &check_name))
        .await
        .map_err(|e| ApiError::internal(format!("check task failed: {e}")))??;
    spawn(state, kind, &name, job)
}

/// The one way every Operation kind starts: claims `key` (a Workspace name or
/// `repo:<name>`; 409 `busy` with the running Operation's id otherwise), runs
/// the job on a blocking thread with a runner that honours the Operation's
/// cancel token, and answers 202 with the new Operation. Because all kinds
/// share this path, `POST /operations/{id}/cancel` works for every one of
/// them, and a canceled Operation is recorded as such.
pub(super) fn spawn(
    state: &AppState,
    kind: &'static str,
    key: &str,
    job: operations::Job,
) -> Accepted {
    let handle = state
        .operations
        .start(kind, key, &state.activity)
        .map_err(|busy| {
            let what = match key.strip_prefix("repo:") {
                Some(repo) => format!("repo '{repo}'"),
                None => format!("workspace '{key}'"),
            };
            ApiError::busy(
                format!("{what} already has operation {} running", busy.operation_id),
                busy.operation_id,
            )
        })?;
    let view = state.operations.view(handle.id());
    let mgr = ConfigManager::with_base_dir(state.base_dir.clone());
    let runner = state.runner.clone();
    let events = state.events.clone();
    let key = key.to_string();
    // Detached on purpose: closing the client connection never stops the work.
    // A panic drops the handle, which records the failure.
    tokio::task::spawn_blocking(move || {
        let runner = runner.make().with_cancel(handle.cancel_token());
        let sink = handle.sink();
        let ctx = RequestContext::new(&sink, job.env);
        let outcome = (job.work)(&mgr, &runner, &ctx);
        // The Daemon's own operations announce their effect directly, before
        // the Operation is marked finished; no scan involved.
        events::announce_outcome(&*events, &mgr, kind, &key, outcome.is_ok());
        match outcome {
            Ok(result) => handle.succeed(result),
            Err(e) => handle.fail_with(&e),
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(view.unwrap_or(serde_json::Value::Null)),
    ))
}

async fn done(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let body = operations::parse_body(&body)?;
    submit(
        &state,
        "done",
        name.clone(),
        operations::done_job(&name, body),
    )
    .await
}

async fn cancel(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let body = operations::parse_body(&body)?;
    submit(
        &state,
        "cancel",
        name.clone(),
        operations::cancel_job(&name, body),
    )
    .await
}

async fn operation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .operations
        .view(&id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("operation '{id}' not found")))
}

/// `GET /events`: Server-Sent Events. `Last-Event-ID` resumes after that
/// sequence number; see `events` for the frames.
async fn event_stream(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let last_event_id = match headers.get("last-event-id") {
        None => None,
        Some(raw) => Some(
            raw.to_str()
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .ok_or_else(|| {
                    ApiError::bad_request("Last-Event-ID must be a non-negative integer")
                })?,
        ),
    };
    let stream = events::subscribe(
        state.events.clone(),
        state.operations.clone(),
        &state.activity,
        state.base_dir.clone(),
        last_event_id,
    )
    .await?;
    Ok(axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15))))
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
    listener: Listener,
    state: AppState,
    idle_timeout: Duration,
    shutdown: Arc<Notify>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let activity = state.activity.clone();
        let hub = state.events.clone();
        let app = router(state);
        let stop = shutdown.clone();
        let done = async move {
            tokio::select! {
                _ = stop.notified() => {}
                _ = wait_for_signal() => {}
                _ = wait_until_idle(activity, idle_timeout) => {
                    tracing::info!("daemon idle; exiting");
                }
            }
            // Open event streams would otherwise hold graceful shutdown open.
            hub.close();
        };
        match listener {
            #[cfg(unix)]
            Listener::Unix(listener) => {
                let listener = tokio::net::UnixListener::from_std(listener)?;
                axum::serve(listener, app)
                    .with_graceful_shutdown(done)
                    .await?;
            }
            #[cfg(windows)]
            Listener::Tcp(listener) => {
                let listener = tokio::net::TcpListener::from_std(listener)?;
                axum::serve(listener, app)
                    .with_graceful_shutdown(done)
                    .await?;
            }
        }
        Ok(())
    })
}

#[cfg(windows)]
async fn wait_for_signal() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

#[cfg(unix)]
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
