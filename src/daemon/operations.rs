//! Operations: tracked executions of state-changing requests (CONTEXT.md).
//!
//! The registry is the single place that knows which Operations exist. It
//! gives each one an identity, decides per Workspace whether a new one may
//! start (busy rule), collects the service-layer events into the Operation's
//! output, keeps the Daemon from idle-exiting while one runs, and drops old
//! records. It lives in memory only.
//!
//! Attachment points for later work: new endpoints call `start` and run their
//! service function with an [`OperationHandle::sink`]; the event stream
//! subscribes where [`OperationRegistry::record_event`] appends; explicit
//! cancel looks the record up by id.

use super::error::{self, ApiError};
use super::{Activity, ActivityGuard};
use crate::config::{ConfigManager, WorkspaceBusy};
use crate::runner::{CommandRunner, CommandSpec, OutputLine};
use crate::service::lifecycle::{
    cancel_workspace, done_workspace, CancelRequest, DoneRequest, UncommittedConfirmationRequired,
};
use crate::service::{EnvSnapshot, EventSink, RequestContext, ServiceEvent};
use anyhow::Result;
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::process::{ExitStatus, Output};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How many finished Operation records are kept, and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    pub max_records: usize,
    pub max_age: Duration,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            max_records: 100,
            max_age: Duration::from_secs(24 * 60 * 60),
        }
    }
}

/// Why an Operation failed. `code` is a stable wire code; `details` is merged
/// into the error object (for example `repos`).
#[derive(Debug, Clone)]
pub struct OperationError {
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

impl OperationError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: error::INTERNAL_ERROR,
            message: message.into(),
            details: None,
        }
    }

    /// Maps a service error to a wire error: lock conflicts become `busy`,
    /// missing confirmation becomes `confirmation_required`, everything else
    /// `operation_failed` with the complete error chain as the message.
    pub fn from_service_error(e: &anyhow::Error) -> Self {
        if let Some(busy) = e.chain().find_map(|c| c.downcast_ref::<WorkspaceBusy>()) {
            return Self {
                code: error::BUSY,
                message: busy.to_string(),
                details: None,
            };
        }
        if let Some(needed) = e
            .chain()
            .find_map(|c| c.downcast_ref::<UncommittedConfirmationRequired>())
        {
            return Self {
                code: error::CONFIRMATION_REQUIRED,
                message: needed.to_string(),
                details: Some(json!({ "repos": needed.repos })),
            };
        }
        Self {
            code: error::OPERATION_FAILED,
            message: format!("{e:#}"),
            details: None,
        }
    }

    fn to_json(&self) -> Value {
        let mut out = json!({ "code": self.code, "message": self.message });
        if let Some(Value::Object(details)) = &self.details {
            for (k, v) in details {
                out[k] = v.clone();
            }
        }
        out
    }
}

enum State {
    Running,
    Succeeded(Value),
    Failed(OperationError),
}

struct Record {
    id: String,
    kind: &'static str,
    workspace: String,
    state: State,
    output: Vec<String>,
    started_at: String,
    finished_at: Option<String>,
    finished: Option<Instant>,
}

impl Record {
    fn view(&self) -> Value {
        let (status, result, error) = match &self.state {
            State::Running => ("running", Value::Null, Value::Null),
            State::Succeeded(v) => ("succeeded", v.clone(), Value::Null),
            State::Failed(e) => ("failed", Value::Null, e.to_json()),
        };
        json!({
            "id": self.id,
            "kind": self.kind,
            "workspace": self.workspace,
            "status": status,
            "output": self.output,
            "result": result,
            "error": error,
            "started_at": self.started_at,
            "finished_at": self.finished_at,
        })
    }
}

/// Another Operation is already running on the Workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    pub operation_id: String,
}

pub struct OperationRegistry {
    records: Mutex<Vec<Record>>,
    retention: Retention,
}

impl OperationRegistry {
    pub fn new(retention: Retention) -> Arc<Self> {
        Arc::new(Self {
            records: Mutex::new(Vec::new()),
            retention,
        })
    }

    /// Registers a running Operation on `workspace`, unless one is already
    /// running there. The check and the registration are one atomic step.
    /// The returned handle keeps the Daemon from idle-exiting.
    pub fn start(
        self: &Arc<Self>,
        kind: &'static str,
        workspace: &str,
        activity: &Arc<Activity>,
    ) -> Result<OperationHandle, Busy> {
        let mut records = self.records.lock().unwrap();
        if let Some(running) = records
            .iter()
            .find(|r| r.workspace == workspace && matches!(r.state, State::Running))
        {
            return Err(Busy {
                operation_id: running.id.clone(),
            });
        }
        let id = new_id();
        records.push(Record {
            id: id.clone(),
            kind,
            workspace: workspace.to_string(),
            state: State::Running,
            output: Vec::new(),
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: None,
            finished: None,
        });
        self.prune(&mut records);
        Ok(OperationHandle {
            registry: self.clone(),
            id,
            finished: false,
            _activity: activity.begin_operation(),
        })
    }

    /// JSON view of one Operation, or `None` when unknown or already dropped.
    pub fn view(&self, id: &str) -> Option<Value> {
        let mut records = self.records.lock().unwrap();
        self.prune(&mut records);
        records.iter().find(|r| r.id == id).map(Record::view)
    }

    /// Appends a service event to the Operation's output. This is where an
    /// event stream will also fan the event out.
    pub fn record_event(&self, id: &str, event: &ServiceEvent) {
        let line = match event {
            ServiceEvent::Started {
                operation,
                workspace,
            } => format!("{operation} {workspace}"),
            ServiceEvent::Step(message)
            | ServiceEvent::Log(message)
            | ServiceEvent::Finished {
                summary: message, ..
            } => message.clone(),
            ServiceEvent::StepStarted(name) => format!("==> {name}"),
            ServiceEvent::StepFinished { name, ok } => {
                format!("<== {name} {}", if *ok { "ok" } else { "failed" })
            }
            ServiceEvent::Warning(message) => format!("warning: {message}"),
        };
        let mut records = self.records.lock().unwrap();
        if let Some(record) = records.iter_mut().find(|r| r.id == id) {
            record.output.push(line);
        }
    }

    fn finish(&self, id: &str, state: State) {
        let mut records = self.records.lock().unwrap();
        if let Some(record) = records.iter_mut().find(|r| r.id == id) {
            record.state = state;
            record.finished_at = Some(chrono::Utc::now().to_rfc3339());
            record.finished = Some(Instant::now());
        }
        self.prune(&mut records);
    }

    /// Drops finished records past the age limit, then the oldest finished
    /// ones beyond the count limit. Running Operations are never dropped.
    fn prune(&self, records: &mut Vec<Record>) {
        let max_age = self.retention.max_age;
        records.retain(|r| r.finished.is_none_or(|at| at.elapsed() <= max_age));
        let finished = records.iter().filter(|r| r.finished.is_some()).count();
        let mut excess = finished.saturating_sub(self.retention.max_records);
        records.retain(|r| {
            if excess > 0 && r.finished.is_some() {
                excess -= 1;
                false
            } else {
                true
            }
        });
    }
}

fn new_id() -> String {
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("op-{hex}")
}

/// Owns one running Operation. Finishing consumes it; dropping it unfinished
/// (a panic in the work) records a failure so the Workspace never stays busy.
pub struct OperationHandle {
    registry: Arc<OperationRegistry>,
    id: String,
    finished: bool,
    _activity: ActivityGuard,
}

impl OperationHandle {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Sink that feeds service events into this Operation's output.
    pub fn sink(&self) -> OperationSink {
        OperationSink {
            registry: self.registry.clone(),
            id: self.id.clone(),
        }
    }

    pub fn succeed(mut self, result: Value) {
        self.finished = true;
        self.registry.finish(&self.id, State::Succeeded(result));
    }

    pub fn fail(mut self, error: OperationError) {
        self.finished = true;
        self.registry.finish(&self.id, State::Failed(error));
    }
}

impl Drop for OperationHandle {
    fn drop(&mut self) {
        if !self.finished {
            self.registry.finish(
                &self.id,
                State::Failed(OperationError::internal(
                    "operation ended without a result (the worker panicked)",
                )),
            );
        }
    }
}

pub struct OperationSink {
    registry: Arc<OperationRegistry>,
    id: String,
}

impl EventSink for OperationSink {
    fn emit(&self, event: ServiceEvent) {
        self.registry.record_event(&self.id, &event);
    }
}

/// Boxed runner usable where the service layer wants `R: CommandRunner`.
pub struct DynRunner(pub Box<dyn CommandRunner>);

impl CommandRunner for DynRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        self.0.run(spec)
    }

    fn run_streaming(
        &self,
        spec: &CommandSpec,
        on_line: &mut dyn FnMut(OutputLine),
    ) -> Result<Output> {
        self.0.run_streaming(spec, on_line)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus> {
        self.0.run_interactive(spec)
    }
}

/// Creates the runner an Operation uses; one per Operation, on its worker
/// thread. Tests substitute a double.
#[derive(Clone)]
pub struct RunnerFactory(Arc<dyn Fn() -> Box<dyn CommandRunner> + Send + Sync>);

impl RunnerFactory {
    pub fn new(make: impl Fn() -> Box<dyn CommandRunner> + Send + Sync + 'static) -> Self {
        Self(Arc::new(make))
    }

    pub fn real() -> Self {
        Self::new(|| Box::new(crate::runner::RealRunner))
    }

    pub fn make(&self) -> DynRunner {
        DynRunner((self.0)())
    }
}

impl std::fmt::Debug for RunnerFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunnerFactory")
    }
}

// ---- request bodies -------------------------------------------------------

/// Parses an optional JSON body; an empty body means all defaults.
pub fn parse_body<T: for<'de> Deserialize<'de> + Default>(raw: &[u8]) -> Result<T, ApiError> {
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(raw)
        .map_err(|e| ApiError::bad_request(format!("invalid request body: {e}")))
}

/// Environment snapshot sent by the client. Absent: the Daemon's own
/// whitelisted environment (never an empty one, which would strip PATH).
fn snapshot(env: Option<BTreeMap<String, String>>) -> EnvSnapshot {
    match env {
        Some(vars) => EnvSnapshot::from_pairs(vars),
        None => EnvSnapshot::capture(),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DoneBody {
    pub env: Option<BTreeMap<String, String>>,
    pub no_merge: bool,
    pub no_clean: bool,
    pub push: bool,
    pub strategy: Option<String>,
    pub force: bool,
    pub skip_hooks: bool,
    pub dry_run: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CancelBody {
    pub env: Option<BTreeMap<String, String>>,
    pub no_clean: bool,
    pub force: bool,
    pub skip_hooks: bool,
    /// The caller already confirmed continuing despite uncommitted changes.
    pub confirmed_uncommitted: bool,
}

/// Work an endpoint hands to the registry: the environment snapshot plus the
/// synchronous service call that produces the Operation's result.
pub struct Job {
    pub env: EnvSnapshot,
    pub work:
        Box<dyn FnOnce(&ConfigManager, &DynRunner, &RequestContext<'_>) -> Result<Value> + Send>,
}

fn outcome_json(outcome: &crate::service::lifecycle::LifecycleOutcome) -> Value {
    json!({ "workspace": outcome.workspace, "dry_run": outcome.dry_run })
}

pub fn done_job(name: &str, body: DoneBody) -> Job {
    let req = DoneRequest {
        name: name.to_string(),
        no_merge: body.no_merge,
        no_clean: body.no_clean,
        push: body.push,
        strategy: body.strategy,
        force: body.force,
        skip_hooks: body.skip_hooks,
        dry_run: body.dry_run,
    };
    Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, runner, ctx| {
            Ok(outcome_json(&done_workspace(mgr, runner, ctx, &req)?))
        }),
    }
}

pub fn cancel_job(name: &str, body: CancelBody) -> Job {
    let req = CancelRequest {
        name: name.to_string(),
        no_clean: body.no_clean,
        force: body.force,
        skip_hooks: body.skip_hooks,
        confirmed_uncommitted: body.confirmed_uncommitted,
    };
    Job {
        env: snapshot(body.env),
        work: Box::new(move |mgr, runner, ctx| {
            Ok(outcome_json(&cancel_workspace(mgr, runner, ctx, &req)?))
        }),
    }
}
