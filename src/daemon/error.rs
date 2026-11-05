//! Stable error envelope: `{"error": {"code": "<code>", "message": "<text>"}}`.
//!
//! Codes are part of the wire contract. Consumers branch on `code`, never on
//! `message`. New codes may be added; existing ones are never renamed or
//! repurposed (removal requires a protocol version bump).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Malformed request: bad path segment, unknown filter value, and so on. HTTP 400.
pub const BAD_REQUEST: &str = "bad_request";
/// Missing or invalid bearer token. HTTP 401.
pub const UNAUTHORIZED: &str = "unauthorized";
/// No such endpoint or resource. HTTP 404.
pub const NOT_FOUND: &str = "not_found";
/// Reserved for later tickets: the target is busy with another Operation.
/// HTTP 409. Nothing returns it yet.
pub const BUSY: &str = "busy";
/// Unexpected server-side failure. HTTP 500.
pub const INTERNAL_ERROR: &str = "internal_error";
/// The client declared a protocol version below the Daemon's minimum
/// compatible version; the client must upgrade. HTTP 426.
pub const CLIENT_TOO_OLD: &str = "client_too_old";
/// The client declared a protocol version above the Daemon's; the Daemon (or
/// the client's target) must be upgraded. HTTP 426.
pub const CLIENT_TOO_NEW: &str = "client_too_new";

/// Every code the Daemon may return, for documentation and tests.
pub const ERROR_CODES: &[&str] = &[
    BAD_REQUEST,
    UNAUTHORIZED,
    NOT_FOUND,
    BUSY,
    INTERNAL_ERROR,
    CLIENT_TOO_OLD,
    CLIENT_TOO_NEW,
];

/// Request header in which a client declares its protocol version.
pub const PROTOCOL_HEADER: &str = "x-zootree-protocol-version";

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, BAD_REQUEST, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, NOT_FOUND, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_ERROR, message)
    }
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
