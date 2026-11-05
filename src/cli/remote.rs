//! Command-line client mode (ADR-0006).
//!
//! When a compatible Daemon is running, state-changing and read commands go
//! through it: the command subscribes to the event stream, submits its
//! request with the caller's environment snapshot, prints the Operation's
//! progress live and waits for the result. Output matches local execution.
//!
//! Rules this module enforces:
//! - The Daemon is used only when the discovery file exists and the
//!   three-state probe says it accepts connections (`daemon::inspect`) and its
//!   protocol range and capabilities fit this client. Anything else (absent,
//!   not listening, unproven, incompatible) means local execution, with a
//!   notice only for an incompatible protocol.
//! - Nothing here starts a Daemon.
//! - A failure to reach the Daemon before the request has been written out
//!   falls back to local execution. Once the whole request is written, any
//!   failure (no answer, a cut connection, an unreadable answer) is an error
//!   that points at `zootree operations`: the Daemon may have accepted it, and
//!   running it locally too could do the work twice. After acceptance the
//!   command never falls back either; it waits, or fails.
//! - Ctrl-C only stops waiting. `zootree operations cancel <id>` stops the
//!   Operation.

use crate::config::ConfigManager;
use crate::daemon::client::{self, EventStream, HttpStatusError, RequestSent};
use crate::daemon::PROTOCOL_VERSION;
use crate::daemon::{self, DaemonOptions, DaemonState};
use crate::service::start::AttachRequest;
use crate::service::EnvSnapshot;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Request header in which this client declares its protocol version.
pub use crate::daemon::client::PROTOCOL_HEADER;
/// What the Daemon must offer for the client mode to be used.
const REQUIRED_CAPABILITIES: [&str; 3] = ["read_api", "operations", "events"];
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the event stream may stay silent between checks of the interrupt flag.
const POLL_SLICE: Duration = Duration::from_millis(200);

// ---- discovery ------------------------------------------------------------

/// A Daemon this command line may use.
#[derive(Debug, Clone)]
pub struct DaemonClient {
    socket: PathBuf,
    token: String,
}

/// Result of looking for a Daemon.
#[derive(Debug)]
pub enum Connection {
    Daemon(DaemonClient),
    /// Run locally. `notice` explains why when the user should be told.
    Local {
        notice: Option<String>,
    },
}

/// Finds a usable Daemon for `base_dir`. Never starts one.
pub fn discover(base_dir: &Path) -> Connection {
    let local = |reason: String| {
        tracing::debug!("running locally: {reason}");
        Connection::Local { notice: None }
    };
    let options = DaemonOptions::new(base_dir.to_path_buf());
    let paths = match options.paths() {
        Ok(paths) => paths,
        Err(e) => return local(format!("{e:#}")),
    };
    // The common case, no Daemon at all, costs two stat calls.
    if !paths.discovery.exists() && !paths.socket.exists() {
        return local("no daemon endpoint".into());
    }
    let discovery = match daemon::inspect(&paths) {
        Ok(DaemonState::Running(discovery)) => discovery,
        Ok(DaemonState::NotRunning) => return local("daemon is not listening".into()),
        Ok(DaemonState::Unknown(reason)) => {
            return local(format!("daemon state could not be proven: {reason}"))
        }
        Err(e) => return local(format!("{e:#}")),
    };
    let client = DaemonClient {
        socket: discovery.endpoint,
        token: discovery.token,
    };
    // `/status` is exempt from version negotiation, so it always answers.
    let status = match client.call("GET", "/status", None) {
        Ok(status) => status,
        Err(e) => return local(format!("daemon status unavailable: {e}")),
    };
    match check_status(&status) {
        Ok(()) => Connection::Daemon(client),
        Err(Mismatch::Incompatible(notice)) => Connection::Local {
            notice: Some(notice),
        },
        Err(Mismatch::Missing(reason)) => local(reason),
    }
}

/// Like [`discover`], printing the incompatibility notice (to stderr, so
/// machine-readable stdout stays clean).
pub fn connect(config_mgr: &ConfigManager) -> Option<DaemonClient> {
    match discover(&config_mgr.base_dir) {
        Connection::Daemon(client) => Some(client),
        Connection::Local { notice } => {
            if let Some(notice) = notice {
                eprintln!("{notice}");
            }
            None
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mismatch {
    /// Protocol versions do not overlap; the user is told.
    Incompatible(String),
    /// A needed capability is missing; silently local.
    Missing(String),
}

/// Judges a `/status` body against this client's protocol range.
pub fn check_status(status: &Value) -> Result<(), Mismatch> {
    let daemon_protocol = status["protocol_version"].as_u64();
    let min_compatible = status["min_compatible_protocol_version"].as_u64();
    let (Some(daemon_protocol), Some(min_compatible)) = (daemon_protocol, min_compatible) else {
        return Err(Mismatch::Missing(
            "daemon status lacks protocol information".into(),
        ));
    };
    let mine = u64::from(PROTOCOL_VERSION);
    if mine < min_compatible || mine > daemon_protocol {
        return Err(Mismatch::Incompatible(incompatible_notice(
            status["version"].as_str().unwrap_or("?"),
            daemon_protocol,
            min_compatible,
        )));
    }
    let offered = status["capabilities"].as_array();
    for needed in REQUIRED_CAPABILITIES {
        if !offered.is_some_and(|all| all.iter().any(|c| c == needed)) {
            return Err(Mismatch::Missing(format!(
                "daemon does not offer the '{needed}' capability"
            )));
        }
    }
    Ok(())
}

fn incompatible_notice(daemon_version: &str, daemon_protocol: u64, min_compatible: u64) -> String {
    let hint = if u64::from(PROTOCOL_VERSION) > daemon_protocol {
        "restart the daemon with this version (`zootree daemon stop`, then `zootree daemon start`)"
    } else {
        "upgrade zootree"
    };
    format!(
        "warning: the running zootree daemon (version {daemon_version}) speaks protocol {min_compatible}..={daemon_protocol}, \
         this zootree speaks protocol {PROTOCOL_VERSION}; running locally instead. To use the daemon, {hint}."
    )
}

// ---- plain requests -------------------------------------------------------

/// The Daemon answered with an error envelope.
#[derive(Debug, Clone)]
pub struct ApiFailure {
    pub http_status: u16,
    pub code: String,
    pub message: String,
    pub operation_id: Option<String>,
}

impl std::fmt::Display for ApiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiFailure {}

impl ApiFailure {
    fn is_incompatible(&self) -> bool {
        matches!(
            self.code.as_str(),
            crate::daemon::error::CLIENT_TOO_OLD
                | crate::daemon::error::CLIENT_TOO_NEW
                | crate::daemon::error::PROTOCOL_VERSION_REQUIRED
        )
    }
}

#[derive(Debug)]
enum CallError {
    /// The Daemon could not be reached or its answer was unreadable.
    Transport(anyhow::Error),
    Api(ApiFailure),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Transport(e) => write!(f, "{e:#}"),
            CallError::Api(e) => write!(f, "{e}"),
        }
    }
}

/// Why the Daemon cannot serve a request that has not been accepted yet.
#[derive(Debug)]
enum Unavailable {
    Incompatible(ApiFailure),
    Down(String),
}

impl Unavailable {
    /// Local execution takes over; only a protocol mismatch is worth saying.
    fn fall_back(self) {
        match self {
            Unavailable::Incompatible(failure) => {
                eprintln!(
                    "warning: the running zootree daemon rejected this client ({}); running locally instead.",
                    failure.message
                );
            }
            Unavailable::Down(reason) => {
                tracing::debug!("daemon unavailable, running locally: {reason}")
            }
        }
    }
}

impl DaemonClient {
    pub fn new(socket: PathBuf, token: String) -> Self {
        Self { socket, token }
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, CallError> {
        let version = PROTOCOL_VERSION.to_string();
        let body = body.map(Value::to_string);
        let response = client::request_with_body(
            &self.socket,
            method,
            path,
            Some(&self.token),
            &[(PROTOCOL_HEADER, version.as_str())],
            body.as_deref(),
            REQUEST_TIMEOUT,
        )
        .map_err(CallError::Transport)?;
        if (200..300).contains(&response.status) {
            if response.body.trim().is_empty() {
                return Ok(Value::Null);
            }
            return response
                .json()
                .map_err(|e| CallError::Transport(e.context(RequestSent)));
        }
        let parsed = serde_json::from_str::<Value>(&response.body).unwrap_or(Value::Null);
        let error = &parsed["error"];
        Err(CallError::Api(ApiFailure {
            http_status: response.status,
            code: error["code"].as_str().unwrap_or("unknown").to_string(),
            message: error["message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("daemon answered HTTP {}", response.status)),
            operation_id: error["operation_id"].as_str().map(str::to_string),
        }))
    }

    /// Reads `path`. `None` means "do it locally": the Daemon is unreachable,
    /// rejected this client, or answered an error (local execution then
    /// produces the authoritative result or error).
    pub fn read(&self, path: &str) -> Option<Value> {
        match self.call("GET", path, None) {
            Ok(value) => Some(value),
            Err(CallError::Transport(e)) => {
                Unavailable::Down(format!("{e:#}")).fall_back();
                None
            }
            Err(CallError::Api(failure)) if failure.is_incompatible() => {
                Unavailable::Incompatible(failure).fall_back();
                None
            }
            Err(CallError::Api(failure)) => {
                tracing::debug!(
                    "daemon read {path} failed ({}), running locally",
                    failure.message
                );
                None
            }
        }
    }

    /// Asks the Daemon to stop an Operation (`POST /operations/{id}/cancel`).
    pub fn cancel_operation(&self, id: &str) -> Result<Value> {
        self.call("POST", &format!("/operations/{id}/cancel"), None)
            .map_err(|e| match e {
                CallError::Api(failure) => anyhow::Error::new(failure),
                CallError::Transport(e) => e.context("could not reach the zootree daemon"),
            })
    }

    /// Current view of an Operation.
    pub fn operation(&self, id: &str) -> Result<Value> {
        self.call("GET", &format!("/operations/{id}"), None)
            .map_err(|e| match e {
                CallError::Api(failure) => anyhow::Error::new(failure),
                CallError::Transport(e) => e.context("could not reach the zootree daemon"),
            })
    }

    pub fn open_event_stream(&self) -> Result<EventStream> {
        self.open_stream().map_err(|unavailable| match unavailable {
            Unavailable::Incompatible(failure) => anyhow::Error::new(failure),
            Unavailable::Down(reason) => anyhow::anyhow!(reason),
        })
    }

    fn open_stream(&self) -> Result<EventStream, Unavailable> {
        let version = PROTOCOL_VERSION.to_string();
        let mut stream = EventStream::connect(
            &self.socket,
            Some(&self.token),
            &[(PROTOCOL_HEADER, version.as_str())],
            None,
        )
        .map_err(|e| {
            let text = format!("{e:#}");
            let rejected = e
                .downcast_ref::<HttpStatusError>()
                .and_then(|status| Some((status.status, status.error_code()?)))
                .filter(|(_, code)| {
                    matches!(
                        code.as_str(),
                        crate::daemon::error::CLIENT_TOO_OLD
                            | crate::daemon::error::CLIENT_TOO_NEW
                            | crate::daemon::error::PROTOCOL_VERSION_REQUIRED
                    )
                });
            match rejected {
                Some((http_status, code)) => Unavailable::Incompatible(ApiFailure {
                    http_status,
                    code,
                    message: text,
                    operation_id: None,
                }),
                None => Unavailable::Down(text),
            }
        })?;
        // Subscribing before submitting means no event of ours can be missed.
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            match stream.next_frame(POLL_SLICE) {
                Ok(Some(frame)) if frame.event == "ready" => return Ok(stream),
                Ok(Some(_)) => {}
                Ok(None) => return Err(Unavailable::Down("event stream closed".into())),
                Err(e) if is_timeout(&e) && Instant::now() < deadline => {}
                Err(e) => return Err(Unavailable::Down(format!("{e:#}"))),
            }
        }
    }
}

fn is_timeout(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            )
        })
    })
}

// ---- operations -----------------------------------------------------------

/// A printed line, mirroring what `CliEventSink` does locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Emit {
    /// Progress line: stdout.
    Step(String),
    /// Non-fatal problem: the log at warn level.
    Warning(String),
    /// Child output and step bookkeeping: shown with `-v` only.
    Debug(String),
}

/// How an Operation ended.
#[derive(Debug, Clone)]
pub struct OperationEnd {
    pub id: String,
    /// `succeeded`, `failed` or `canceled`.
    pub status: String,
    pub result: Value,
    pub error: Value,
    /// Details of a stopped Operation (`canceled` status only).
    pub canceled: Value,
}

#[derive(Debug)]
pub enum Followed {
    Ended(Box<OperationEnd>),
    /// The caller asked to stop waiting; the Operation keeps running.
    Interrupted,
}

/// Waits for Operation `id`, reporting its progress through `emit`. Reads the
/// event stream; if the stream breaks or has to resync, polls the Operation
/// instead. Checks `interrupted` at least every 200 ms.
pub fn follow_operation(
    client: &DaemonClient,
    stream: &mut EventStream,
    id: &str,
    emit: &mut dyn FnMut(Emit),
    interrupted: &dyn Fn() -> bool,
) -> Result<Followed> {
    loop {
        if interrupted() {
            return Ok(Followed::Interrupted);
        }
        let frame = match stream.next_frame(POLL_SLICE) {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => {
                tracing::debug!("event stream failed, polling instead: {e:#}");
                break;
            }
        };
        if frame.event == "resync" {
            break;
        }
        if frame.data["operation_id"].as_str() != Some(id) {
            continue;
        }
        let data = &frame.data;
        let text = |key: &str| data[key].as_str().unwrap_or_default().to_string();
        match frame.event.as_str() {
            "operation.progress" => match data["phase"].as_str().unwrap_or_default() {
                "step" => emit(Emit::Step(text("message"))),
                "warning" => emit(Emit::Warning(text("message"))),
                "step_started" => emit(Emit::Debug(format!("step '{}' started", text("message")))),
                "step_finished" => emit(Emit::Debug(format!(
                    "step '{}' finished (ok: {})",
                    text("message"),
                    data["ok"].as_bool().unwrap_or(false)
                ))),
                _ => {}
            },
            "operation.log" => emit(Emit::Debug(text("message"))),
            "operation.finished" => {
                let status = text("status");
                let canceled = if status == "canceled" {
                    data["result"].clone()
                } else {
                    Value::Null
                };
                return Ok(Followed::Ended(Box::new(OperationEnd {
                    id: id.to_string(),
                    status,
                    result: data["result"].clone(),
                    error: data["error"].clone(),
                    canceled,
                })));
            }
            _ => {}
        }
    }
    poll_operation(client, id, interrupted)
}

fn poll_operation(
    client: &DaemonClient,
    id: &str,
    interrupted: &dyn Fn() -> bool,
) -> Result<Followed> {
    loop {
        if interrupted() {
            return Ok(Followed::Interrupted);
        }
        let view = client.operation(id).with_context(|| {
            format!("lost contact with the zootree daemon while operation {id} was running")
        })?;
        let status = view["status"].as_str().unwrap_or("running");
        if status != "running" {
            return Ok(Followed::Ended(Box::new(OperationEnd {
                id: id.to_string(),
                status: status.to_string(),
                result: view["result"].clone(),
                error: view["error"].clone(),
                canceled: view["canceled"].clone(),
            })));
        }
        std::thread::sleep(POLL_SLICE);
    }
}

/// The Operation ended in `failed`. `code` is the wire code
/// (`confirmation_required`, `busy`, `operation_failed`, ...).
#[derive(Debug, Clone)]
pub struct OperationFailure {
    pub id: String,
    pub code: String,
    pub message: String,
    /// Repos named by a `confirmation_required` failure.
    pub repos: Vec<String>,
}

impl std::fmt::Display for OperationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for OperationFailure {}

/// The user pressed Ctrl-C while waiting; the Operation was not stopped.
#[derive(Debug, Clone)]
pub struct Interrupted {
    pub operation_id: String,
}

impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "stopped waiting; operation {id} keeps running in the daemon. \
             Stop it with `zootree operations cancel {id}`.",
            id = self.operation_id
        )
    }
}

impl std::error::Error for Interrupted {}

/// Exit code of a command interrupted by Ctrl-C.
pub const INTERRUPTED_EXIT_CODE: i32 = 130;

static SIGINT_SEEN: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    SIGINT_SEEN.store(true, Ordering::SeqCst);
}

/// While alive, Ctrl-C sets a flag instead of killing the process, so the
/// wait can end cleanly with a hint. Restores the previous handler on drop.
struct SigintGuard {
    previous: libc::sighandler_t,
}

impl SigintGuard {
    fn install() -> Self {
        SIGINT_SEEN.store(false, Ordering::SeqCst);
        // SAFETY: the handler only stores to an atomic flag.
        let previous =
            unsafe { libc::signal(libc::SIGINT, on_sigint as *const () as libc::sighandler_t) };
        Self { previous }
    }

    fn seen() -> bool {
        SIGINT_SEEN.load(Ordering::SeqCst)
    }
}

impl Drop for SigintGuard {
    fn drop(&mut self) {
        // SAFETY: restores the handler that was installed before.
        unsafe {
            libc::signal(libc::SIGINT, self.previous);
        }
    }
}

/// Per-command knobs for [`DaemonClient::execute`].
#[derive(Default)]
pub struct ExecOptions {
    /// Progress lines for which this returns true are not printed.
    pub skip_step: Option<fn(&str) -> bool>,
    /// Print the closing summary of `done`/`cancel` (not in a dry run).
    pub print_summary: bool,
}

/// A successfully finished Operation.
#[derive(Debug, Clone)]
pub struct Finished {
    pub id: String,
    pub result: Value,
}

impl DaemonClient {
    /// Submits a state-changing request and waits for the Operation, printing
    /// progress like the local event sink. `Ok(None)` means the Daemon could
    /// not take the request and the caller must run the command locally.
    pub fn execute(
        &self,
        method: &str,
        path: &str,
        mut body: Value,
        options: &ExecOptions,
    ) -> Result<Option<Finished>> {
        let mut stream = match self.open_stream() {
            Ok(stream) => stream,
            Err(unavailable) => {
                unavailable.fall_back();
                return Ok(None);
            }
        };
        if let Value::Object(map) = &mut body {
            map.insert("env".into(), json!(EnvSnapshot::capture().vars()));
        }
        let view = match self.call(method, path, Some(&body)) {
            Ok(view) => view,
            // The whole request went out, so the Daemon may have accepted it.
            // Running it locally now could do the work twice.
            Err(CallError::Transport(e)) if e.downcast_ref::<RequestSent>().is_some() => {
                return Err(e.context(format!(
                    "the request to {path} was sent to the zootree daemon but its answer did not arrive; \
                     it may have been accepted, so it was not run locally. \
                     Check `zootree operations` before trying again"
                )));
            }
            Err(CallError::Transport(e)) => {
                Unavailable::Down(format!("{e:#}")).fall_back();
                return Ok(None);
            }
            Err(CallError::Api(failure)) if failure.is_incompatible() => {
                Unavailable::Incompatible(failure).fall_back();
                return Ok(None);
            }
            Err(CallError::Api(failure)) => return Err(anyhow::Error::new(failure)),
        };
        let id = view["id"]
            .as_str()
            .context("daemon accepted the request without an operation id")?
            .to_string();
        tracing::debug!("daemon accepted operation {id}");

        let guard = SigintGuard::install();
        let skip = options.skip_step;
        let mut emit = |event: Emit| match event {
            Emit::Step(message) => {
                if !skip.is_some_and(|skip| skip(&message)) {
                    println!("{message}")
                }
            }
            Emit::Warning(message) => tracing::warn!("{}", message),
            Emit::Debug(message) => tracing::debug!("{}", message),
        };
        let followed = follow_operation(self, &mut stream, &id, &mut emit, &SigintGuard::seen);
        drop(guard);
        let end = match followed? {
            Followed::Ended(end) => end,
            Followed::Interrupted => {
                return Err(anyhow::Error::new(Interrupted { operation_id: id }))
            }
        };

        match end.status.as_str() {
            "succeeded" => {
                if options.print_summary && end.result["dry_run"] != json!(true) {
                    if let Some(summary) = self.summary_line(&id) {
                        println!("{summary}");
                    }
                }
                Ok(Some(Finished {
                    id,
                    result: end.result,
                }))
            }
            "canceled" => anyhow::bail!("{}", describe_canceled(&id, &end.canceled)),
            _ => Err(anyhow::Error::new(OperationFailure {
                id,
                code: end.error["code"].as_str().unwrap_or("unknown").to_string(),
                message: end.error["message"]
                    .as_str()
                    .unwrap_or("operation failed")
                    .to_string(),
                repos: end.error["repos"]
                    .as_array()
                    .map(|repos| {
                        repos
                            .iter()
                            .filter_map(|r| r.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            })),
        }
    }

    /// The closing message of `done`/`cancel` is the last line of the
    /// Operation's output (the event stream carries no frame for it).
    fn summary_line(&self, id: &str) -> Option<String> {
        let view = self.operation(id).ok()?;
        view["output"]
            .as_array()?
            .last()?
            .as_str()
            .map(str::to_string)
    }
}

fn describe_canceled(id: &str, info: &Value) -> String {
    let mut text = format!("operation {id} was canceled");
    if let Some(step) = info["stopped_at"].as_str() {
        text.push_str(&format!(" while running '{step}'"));
    }
    if let Some(cleanup) = info["cleanup"].as_str() {
        text.push_str(&format!(" (cleanup: {cleanup})"));
    }
    if let Some(notes) = info["notes"].as_array() {
        for note in notes.iter().filter_map(Value::as_str) {
            text.push_str(&format!("\n  {note}"));
        }
    }
    text
}

// ---- helpers for the command handlers -------------------------------------

/// `run_agent` on the wire: absent, `true` (default agent) or a command.
pub fn run_agent_json(run_agent: &Option<Option<String>>) -> Option<Value> {
    match run_agent {
        None => None,
        Some(Some(command)) if !command.is_empty() => Some(json!(command)),
        Some(_) => Some(json!(true)),
    }
}

/// The attach a start/reopen/create Operation asked the caller to run.
pub fn attach_from_result(result: &Value) -> Option<AttachRequest> {
    let attach = result.get("attach")?;
    let program = attach["program"].as_str()?.to_string();
    let args = attach["args"]
        .as_array()?
        .iter()
        .filter_map(|a| a.as_str().map(str::to_string))
        .collect();
    Some(AttachRequest { program, args })
}
