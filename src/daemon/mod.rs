//! The optional local Daemon (ADR-0006).
//!
//! Single instance per endpoint name, serving HTTP + JSON over a 0600 unix
//! socket. The zootree core stays synchronous; the async runtime only carries
//! the transport.

mod api;
mod auth;
pub mod client;
pub mod endpoint;
pub mod error;
mod mutations;
pub mod operations;
mod server;
mod transport;

use anyhow::{Context, Result};
use endpoint::{Discovery, EndpointPaths, Liveness};
use operations::{OperationRegistry, Retention, RunnerFactory};
use rand::RngCore;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Wire protocol version. Bump only when removing methods, changing field
/// semantics or changing authentication.
pub const PROTOCOL_VERSION: u32 = 1;
/// Oldest client protocol version this Daemon still serves.
pub const MIN_COMPATIBLE_PROTOCOL_VERSION: u32 = 1;
/// Capability strings; new behavior is gated by adding one here.
/// `web` is reserved for a future Web UI transport and must not be declared
/// until that exists (docs/daemon-extension-points.md).
pub const CAPABILITIES: &[&str] = &["status", "read_api", "operations"];
/// Idle time without connections or Operations before the Daemon exits.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct DaemonOptions {
    /// Zootree config directory (`ConfigManager::base_dir`).
    pub base_dir: PathBuf,
    /// Endpoint name, e.g. `v0.1`; different names coexist.
    pub endpoint_name: String,
    pub idle_timeout: Duration,
    /// Builds the command runner each Operation uses; tests substitute a double.
    pub runner: RunnerFactory,
    /// How many finished Operation records to keep, and for how long.
    pub operation_retention: Retention,
}

impl DaemonOptions {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            endpoint_name: endpoint::endpoint_name_for_version(env!("CARGO_PKG_VERSION")),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            runner: RunnerFactory::real(),
            operation_retention: Retention::default(),
        }
    }

    pub fn paths(&self) -> Result<EndpointPaths> {
        EndpointPaths::new(&self.base_dir, &self.endpoint_name)
    }
}

/// Tracks open requests and running Operations for the idle-exit rule.
#[derive(Debug)]
pub struct Activity {
    requests: AtomicUsize,
    operations: AtomicUsize,
    last_busy: Mutex<Instant>,
}

impl Activity {
    fn new() -> Self {
        Self {
            requests: AtomicUsize::new(0),
            operations: AtomicUsize::new(0),
            last_busy: Mutex::new(Instant::now()),
        }
    }

    fn touch(&self) {
        *self.last_busy.lock().unwrap() = Instant::now();
    }

    pub(crate) fn begin_request(self: &Arc<Self>) -> ActivityGuard {
        self.requests.fetch_add(1, Ordering::SeqCst);
        ActivityGuard {
            activity: self.clone(),
            operation: false,
        }
    }

    /// Marks an Operation as running; the Daemon will not idle-exit while the
    /// guard lives.
    pub fn begin_operation(self: &Arc<Self>) -> ActivityGuard {
        self.operations.fetch_add(1, Ordering::SeqCst);
        ActivityGuard {
            activity: self.clone(),
            operation: true,
        }
    }

    fn idle_for(&self) -> Option<Duration> {
        if self.requests.load(Ordering::SeqCst) + self.operations.load(Ordering::SeqCst) > 0 {
            return None;
        }
        Some(self.last_busy.lock().unwrap().elapsed())
    }
}

pub struct ActivityGuard {
    activity: Arc<Activity>,
    operation: bool,
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        let counter = if self.operation {
            &self.activity.operations
        } else {
            &self.activity.requests
        };
        counter.fetch_sub(1, Ordering::SeqCst);
        self.activity.touch();
    }
}

/// Handle to a Daemon running on its own thread.
pub struct DaemonHandle {
    pub socket: PathBuf,
    pub token: String,
    pub paths: EndpointPaths,
    activity: Arc<Activity>,
    shutdown: Arc<tokio::sync::Notify>,
    thread: Option<std::thread::JoinHandle<Result<()>>>,
}

impl DaemonHandle {
    pub fn activity(&self) -> &Arc<Activity> {
        &self.activity
    }

    /// Requests a graceful stop (same effect as `POST /shutdown`).
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
    }

    /// Waits for the Daemon to exit.
    pub fn join(mut self) -> Result<()> {
        match self.thread.take() {
            Some(thread) => thread
                .join()
                .map_err(|_| anyhow::anyhow!("daemon thread panicked"))?,
            None => Ok(()),
        }
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.shutdown.notify_one();
        }
    }
}

/// Everything that must be released or cleaned when the Daemon stops.
struct Claim {
    paths: EndpointPaths,
    token: String,
    _lock: std::fs::File,
}

impl Claim {
    /// Removes only what this Daemon published, while still holding the lock.
    fn release(&self) {
        let ours = matches!(
            Discovery::read(&self.paths.discovery),
            Ok(Some(d)) if d.token == self.token
        );
        if ours {
            let _ = std::fs::remove_file(&self.paths.socket);
            let _ = std::fs::remove_file(&self.paths.discovery);
        }
    }
}

/// Starts a Daemon on its own thread. Returns once it is listening and its
/// discovery file is published, or fails with the reason (for example another
/// instance is already running).
pub fn start(options: DaemonOptions) -> Result<DaemonHandle> {
    let paths = options.paths()?;
    paths.ensure_dir()?;

    // The lock is the single-instance authority; the kernel releases it when
    // the holder dies, however it dies.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&paths.lock)
        .with_context(|| format!("failed to open {}", paths.lock.display()))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            anyhow::bail!(
                "another zootree daemon is already running (endpoint {})",
                paths.socket.display()
            )
        }
        Err(std::fs::TryLockError::Error(e)) => {
            return Err(e).context("failed to lock the daemon lock file")
        }
    }

    take_over_stale_endpoint(&paths.socket)?;

    let listener = std::os::unix::net::UnixListener::bind(&paths.socket)
        .with_context(|| format!("failed to bind {}", paths.socket.display()))?;
    // The directory is 0700 already; tighten the socket itself as well.
    std::fs::set_permissions(&paths.socket, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;

    let token = generate_token();
    let claim = Claim {
        paths: paths.clone(),
        token: token.clone(),
        _lock: lock,
    };
    let discovery = Discovery {
        pid: std::process::id(),
        endpoint: paths.socket.clone(),
        token: token.clone(),
        started_at: chrono::Utc::now().to_rfc3339(),
        protocol_version: PROTOCOL_VERSION,
    };
    if let Err(e) = discovery.write(&paths.discovery) {
        // We just created the socket, so it is ours to remove.
        let _ = std::fs::remove_file(&paths.socket);
        return Err(e);
    }

    let activity = Arc::new(Activity::new());
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let state = server::AppState {
        auth: Arc::new(auth::BearerToken::new(token.clone())),
        base_dir: options.base_dir.clone(),
        pid: discovery.pid,
        started_at: discovery.started_at.clone(),
        activity: activity.clone(),
        shutdown: shutdown.clone(),
        operations: OperationRegistry::new(options.operation_retention),
        runner: options.runner.clone(),
    };
    let idle_timeout = options.idle_timeout;
    let thread = {
        let shutdown = shutdown.clone();
        std::thread::Builder::new()
            .name("zootree-daemon".to_string())
            .spawn(move || {
                let result = server::run(
                    transport::Listener::Unix(listener),
                    state,
                    idle_timeout,
                    shutdown,
                );
                claim.release();
                result
            })?
    };
    Ok(DaemonHandle {
        socket: paths.socket.clone(),
        token,
        paths,
        activity,
        shutdown,
        thread: Some(thread),
    })
}

/// With the lock held no live Daemon of ours exists, but the endpoint may be a
/// leftover or something that is not ours. Remove only a confirmed-dead socket.
fn take_over_stale_endpoint(socket: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(socket) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("failed to stat {}", socket.display())),
    };
    if !metadata.file_type().is_socket() {
        anyhow::bail!(
            "{} exists but is not a socket; refusing to remove it",
            socket.display()
        );
    }
    match endpoint::probe(socket, endpoint::PROBE_TIMEOUT) {
        Liveness::Dead => std::fs::remove_file(socket)
            .with_context(|| format!("failed to remove stale socket {}", socket.display())),
        Liveness::Listening => {
            anyhow::bail!("another process is already serving {}", socket.display())
        }
        Liveness::Unknown(reason) => anyhow::bail!(
            "cannot tell whether {} is in use ({reason}); leaving it untouched",
            socket.display()
        ),
    }
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Observed state of the Daemon for an endpoint, from the client side.
#[derive(Debug)]
pub enum DaemonState {
    Running(Discovery),
    NotRunning,
    /// Present but cannot be judged; treat as possibly running.
    Unknown(String),
}

pub fn inspect(paths: &EndpointPaths) -> Result<DaemonState> {
    let discovery = Discovery::read(&paths.discovery)?;
    let socket = discovery
        .as_ref()
        .map(|d| d.endpoint.clone())
        .unwrap_or_else(|| paths.socket.clone());
    Ok(
        match (endpoint::probe(&socket, endpoint::PROBE_TIMEOUT), discovery) {
            (Liveness::Listening, Some(d)) => DaemonState::Running(d),
            (Liveness::Listening, None) => {
                DaemonState::Unknown("endpoint is serving but has no discovery file".to_string())
            }
            (Liveness::Dead, _) => DaemonState::NotRunning,
            (Liveness::Unknown(reason), _) => DaemonState::Unknown(reason),
        },
    )
}
