//! Endpoint naming, discovery file and the three-state liveness probe.
//!
//! Ownership rules (see docs/adr/0006 and the Orca daemon endpoint notes):
//! only `connection refused` or `missing` prove an endpoint dead; a timeout or
//! a permission error proves nothing and must never lead to deleting it; and
//! nothing is removed unless this module's caller created it.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Longest socket path accepted. `sockaddr_un.sun_path` is 104 bytes on macOS.
const MAX_SOCKET_PATH_LEN: usize = 100;

/// Where one Daemon (identified by its endpoint name) keeps its files.
#[derive(Debug, Clone)]
pub struct EndpointPaths {
    pub dir: PathBuf,
    pub socket: PathBuf,
    pub discovery: PathBuf,
    pub lock: PathBuf,
}

/// Endpoint name for a crate version: `v<major>`, or `v0.<minor>` before 1.0
/// where the minor version is the compatibility boundary.
pub fn endpoint_name_for_version(version: &str) -> String {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("0");
    if major == "0" {
        format!("v0.{}", parts.next().unwrap_or("0"))
    } else {
        format!("v{major}")
    }
}

impl EndpointPaths {
    pub fn new(base_dir: &Path, name: &str) -> Result<Self> {
        let dir = base_dir.join("daemon");
        let socket = dir.join(format!("{name}.sock"));
        if socket.as_os_str().len() > MAX_SOCKET_PATH_LEN {
            anyhow::bail!(
                "daemon socket path is too long for a unix socket ({} bytes): {}",
                socket.as_os_str().len(),
                socket.display()
            );
        }
        Ok(Self {
            discovery: dir.join(format!("{name}.json")),
            lock: dir.join(format!("{name}.lock")),
            socket,
            dir,
        })
    }

    /// Creates the endpoint directory readable only by the current user.
    pub fn ensure_dir(&self) -> Result<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .with_context(|| format!("failed to create {}", self.dir.display()))?;
        // The directory may pre-exist with looser permissions (created by an
        // older run or another tool); the sockets inside rely on it.
        let mode = std::fs::metadata(&self.dir)?.permissions().mode() & 0o777;
        if mode != 0o700 {
            std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

/// Contents of the discovery file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Discovery {
    pub pid: u32,
    pub endpoint: PathBuf,
    pub token: String,
    pub started_at: String,
    pub protocol_version: u32,
}

impl Discovery {
    /// Atomically writes the file with mode 0600.
    pub fn write(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let result = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)
                .with_context(|| format!("failed to create {}", tmp.display()))?;
            file.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)
                .with_context(|| format!("failed to publish {}", path.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    pub fn read(path: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(Some(serde_json::from_str(&content).with_context(|| {
                format!("discovery file {} is malformed", path.display())
            })?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
        }
    }
}

/// Three-state liveness of an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    /// Something accepted our connection.
    Listening,
    /// Connection refused or no such endpoint: confirmed not serving.
    Dead,
    /// Timeout, permission error, or anything else: we cannot tell.
    Unknown(String),
}

/// Maps a connect error to a liveness state. Only refusal and absence prove
/// death.
pub fn classify_connect_error(error: &std::io::Error) -> Liveness {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound => Liveness::Dead,
        _ => Liveness::Unknown(error.to_string()),
    }
}

/// Probes `socket` by connecting to it, giving up (as `Unknown`) after `timeout`.
pub fn probe(socket: &Path, timeout: Duration) -> Liveness {
    let socket = socket.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = match std::os::unix::net::UnixStream::connect(&socket) {
            Ok(_) => Liveness::Listening,
            Err(e) => classify_connect_error(&e),
        };
        let _ = tx.send(result);
    });
    rx.recv_timeout(timeout)
        .unwrap_or_else(|_| Liveness::Unknown("connect timed out".to_string()))
}

/// Default probe timeout.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
