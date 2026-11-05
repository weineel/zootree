use super::{Activity, CAPABILITIES, MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION};
use anyhow::Result;
use axum::extract::{Request, State};
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
    pub pid: u32,
    pub started_at: String,
    pub activity: Arc<Activity>,
    pub shutdown: Arc<Notify>,
}

/// Stable string error code plus a human message.
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": { "code": self.code, "message": self.message } })),
        )
            .into_response()
    }
}

fn router(state: AppState) -> Router {
    // Authentication is its own layer so other listeners (later: loopback TCP)
    // can swap it without touching handlers.
    Router::new()
        .route("/status", get(status))
        .route("/shutdown", post(shutdown))
        .fallback(not_found)
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
            code: "unauthorized",
            message: "missing or invalid token".to_string(),
        }
        .into_response();
    }
    next.run(request).await
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
    ApiError {
        status: StatusCode::NOT_FOUND,
        code: "not_found",
        message: "no such endpoint".to_string(),
    }
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
