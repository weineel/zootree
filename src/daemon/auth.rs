//! Authentication layer. Handlers never see credentials: the router applies
//! one [`Authenticator`] to every request. Another listener (reserved: a
//! loopback TCP listener for a browser) brings its own implementation and
//! changes nothing else. See docs/daemon-extension-points.md.

use super::error::{self, ApiError};
use super::server::AppState;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Decides whether a request may proceed.
pub(super) trait Authenticator: Send + Sync {
    fn authenticate(&self, headers: &HeaderMap) -> Result<(), ApiError>;
}

/// The unix-socket scheme: `Authorization: Bearer <token>` compared with the
/// token published in the 0600 discovery file.
pub(super) struct BearerToken {
    token: String,
}

impl BearerToken {
    pub(super) fn new(token: String) -> Self {
        Self { token }
    }
}

impl Authenticator for BearerToken {
    fn authenticate(&self, headers: &HeaderMap) -> Result<(), ApiError> {
        let presented = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        if presented == Some(self.token.as_str()) {
            Ok(())
        } else {
            Err(ApiError {
                status: StatusCode::UNAUTHORIZED,
                code: error::UNAUTHORIZED,
                message: "missing or invalid token".to_string(),
            })
        }
    }
}

pub(super) async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    match state.auth.authenticate(request.headers()) {
        Ok(()) => next.run(request).await,
        Err(e) => e.into_response(),
    }
}
