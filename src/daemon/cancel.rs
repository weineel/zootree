//! Explicit cancel of a running Operation (`POST /operations/{id}/cancel`).
//!
//! Closing a client connection never stops an Operation; only this request
//! does. Cancelling is best effort: the command running now is killed (see
//! [`crate::runner::CancelToken`]) and the service function unwinds through
//! its own rollback (for example a start removes the worktrees it created).
//! Work that is not a child process, and a command that finishes first, are
//! not interrupted, so the Operation may still end `succeeded` or `failed`.
//! When it ends `canceled`, the record says where it stopped.

use super::error::ApiError;
use super::server::AppState;
use crate::runner::CancelToken;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

/// Describes how a canceled Operation ended. `step` is the last step the
/// service function reported; `error` is what the work failed with.
pub(super) fn canceled_info(
    token: &CancelToken,
    kind: &str,
    step: Option<&str>,
    error: &anyhow::Error,
) -> Option<Value> {
    let command = token.interrupted_command()?;
    let detail = format!("{error:#}");
    let (cleanup, mut notes) = match kind {
        "start" if detail.contains("rollback failed") => (
            "rollback_failed",
            vec![
                "start rollback did not complete; check the worktrees named in detail".to_string(),
            ],
        ),
        "start" => (
            "rolled_back",
            vec!["worktrees created by this start were removed".to_string()],
        ),
        _ => (
            "none",
            vec![
                "no rollback exists for this operation; steps completed before the stop stay applied"
                    .to_string(),
            ],
        ),
    };
    if step == Some("merge") {
        notes.push(
            "the merge was killed mid-way; the target repository may hold a partially applied or in-progress merge, inspect it with git status"
                .to_string(),
        );
    }
    notes.push("the interrupted command was killed, its own effects may be partial".to_string());
    Some(json!({
        "stopped_at": step,
        "interrupted_command": command,
        "cleanup": cleanup,
        "notes": notes,
        "detail": detail,
    }))
}

pub(super) async fn cancel_operation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Some((was_running, operation)) = state.operations.request_cancel(&id) else {
        return Err(ApiError::not_found(format!("operation '{id}' not found")));
    };
    if was_running {
        Ok((
            StatusCode::ACCEPTED,
            Json(json!({
                "id": id,
                "outcome": "cancel_requested",
                "status": "running",
                "operation": operation,
            })),
        ))
    } else {
        // Idempotent: cancelling a finished Operation is not an error.
        Ok((
            StatusCode::OK,
            Json(json!({
                "id": id,
                "outcome": "already_finished",
                "status": operation["status"],
                "operation": operation,
            })),
        ))
    }
}
