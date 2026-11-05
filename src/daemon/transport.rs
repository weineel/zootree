//! Listener transports. The router and handlers are transport-agnostic; a
//! transport only supplies the accepted connections. Today there is exactly
//! one: a 0600 unix socket. A loopback TCP transport (for a future Web UI)
//! would be a second variant here plus its own [`super::auth::Authenticator`];
//! it is deliberately not implemented and opens no port.
//! See docs/daemon-extension-points.md.

/// A bound, non-blocking listener ready to be served.
pub(super) enum Listener {
    Unix(std::os::unix::net::UnixListener),
}
