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

use super::cancel;
use super::error::{self, ApiError};
use super::events::{DaemonEvent, EventHub, EventProducer};
use super::{Activity, ActivityGuard};
use crate::config::{ConfigManager, WorkspaceBusy};
use crate::runner::{CancelToken, CommandRunner, CommandSpec, OutputLine};
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
    /// Stopped by an explicit cancel request; see [`cancel::canceled_info`].
    Canceled(Value),
}

struct Record {
    id: String,
    kind: &'static str,
    workspace: String,
    state: State,
    output: Vec<String>,
    cancel: CancelToken,
    /// Last step the service function reported.
    step: Option<String>,
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
            State::Canceled(_) => ("canceled", Value::Null, Value::Null),
        };
        let canceled = match &self.state {
            State::Canceled(info) => info.clone(),
            _ => Value::Null,
        };
        json!({
            "id": self.id,
            "kind": self.kind,
            "workspace": self.workspace,
            "status": status,
            "output": self.output,
            "result": result,
            "error": error,
            "cancel_requested": self.cancel.is_requested(),
            "canceled": canceled,
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
    events: Arc<EventHub>,
}

impl OperationRegistry {
    pub fn new(retention: Retention, events: Arc<EventHub>) -> Arc<Self> {
        Arc::new(Self {
            records: Mutex::new(Vec::new()),
            retention,
            events,
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
        let started_at = chrono::Utc::now().to_rfc3339();
        self.events.publish(DaemonEvent::OperationStarted {
            operation_id: id.clone(),
            kind,
            workspace: workspace.to_string(),
            started_at: started_at.clone(),
        });
        records.push(Record {
            id: id.clone(),
            kind,
            workspace: workspace.to_string(),
            state: State::Running,
            output: Vec::new(),
            cancel: CancelToken::new(),
            step: None,
            started_at,
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

    /// Asks a running Operation to stop. Returns whether it was still
    /// running, plus its current view; `None` when unknown or dropped.
    pub fn request_cancel(&self, id: &str) -> Option<(bool, Value)> {
        let mut records = self.records.lock().unwrap();
        self.prune(&mut records);
        let record = records.iter().find(|r| r.id == id)?;
        let running = matches!(record.state, State::Running);
        if running {
            record.cancel.request();
        }
        Some((running, record.view()))
    }

    /// Whether an Operation is running on `key` (a Workspace name or
    /// `repo:<name>`).
    pub fn is_running(&self, key: &str) -> bool {
        let records = self.records.lock().unwrap();
        records
            .iter()
            .any(|r| r.workspace == key && matches!(r.state, State::Running))
    }

    /// Views of the Operations still running (for the event stream snapshot).
    pub fn running_views(&self) -> Vec<Value> {
        let records = self.records.lock().unwrap();
        records
            .iter()
            .filter(|r| matches!(r.state, State::Running))
            .map(Record::view)
            .collect()
    }

    /// Appends a service event to the Operation's output and fans it out to
    /// the event stream as `operation.progress` / `operation.log`.
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
            if let ServiceEvent::StepStarted(step) = event {
                record.step = Some(step.clone());
            } else if let ServiceEvent::Step(step) = event {
                record.step = Some(step.trim().to_string());
            }
            if let Some(event) = stream_event(id, &record.workspace, event) {
                self.events.publish(event);
            }
            record.output.push(line);
        }
    }

    /// Ends an Operation whose work failed: `canceled` when an explicit
    /// cancel interrupted a command, `failed` otherwise.
    fn finish_failed(&self, id: &str, error: &anyhow::Error) {
        let state = {
            let records = self.records.lock().unwrap();
            records
                .iter()
                .find(|r| r.id == id)
                .and_then(|r| cancel::canceled_info(&r.cancel, r.kind, r.step.as_deref(), error))
                .map(State::Canceled)
        };
        self.finish(
            id,
            state.unwrap_or_else(|| State::Failed(OperationError::from_service_error(error))),
        );
    }

    fn finish(&self, id: &str, state: State) {
        let mut records = self.records.lock().unwrap();
        if let Some(record) = records.iter_mut().find(|r| r.id == id) {
            record.state = state;
            record.finished_at = Some(chrono::Utc::now().to_rfc3339());
            record.finished = Some(Instant::now());
            let (status, result, error) = match &record.state {
                State::Succeeded(v) => ("succeeded", v.clone(), Value::Null),
                State::Failed(e) => ("failed", Value::Null, e.to_json()),
                State::Running => ("running", Value::Null, Value::Null),
                // The cancel details (stopped step, cleanup outcome) travel as the result.
                State::Canceled(info) => ("canceled", info.clone(), Value::Null),
            };
            self.events.publish(DaemonEvent::OperationFinished {
                operation_id: record.id.clone(),
                kind: record.kind,
                workspace: record.workspace.clone(),
                status,
                result,
                error,
                finished_at: record.finished_at.clone(),
            });
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

/// Service event as stream event. Start and finish are covered by
/// `operation.started` / `operation.finished`, so they map to nothing.
fn stream_event(id: &str, workspace: &str, event: &ServiceEvent) -> Option<DaemonEvent> {
    let progress = |phase, message: &String, ok| DaemonEvent::OperationProgress {
        operation_id: id.to_string(),
        workspace: workspace.to_string(),
        phase,
        message: message.clone(),
        ok,
    };
    match event {
        ServiceEvent::Started { .. } | ServiceEvent::Finished { .. } => None,
        ServiceEvent::Step(m) => Some(progress("step", m, None)),
        ServiceEvent::StepStarted(n) => Some(progress("step_started", n, None)),
        ServiceEvent::StepFinished { name, ok } => Some(progress("step_finished", name, Some(*ok))),
        ServiceEvent::Warning(m) => Some(progress("warning", m, None)),
        ServiceEvent::Log(m) => Some(DaemonEvent::OperationLog {
            operation_id: id.to_string(),
            workspace: workspace.to_string(),
            message: m.clone(),
        }),
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

    /// Token an explicit cancel request trips; the Operation's runner honours it.
    pub fn cancel_token(&self) -> CancelToken {
        self.registry
            .records
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.id == self.id)
            .map(|r| r.cancel.clone())
            .unwrap_or_default()
    }

    /// Ends the Operation after its work returned `error`.
    pub fn fail_with(mut self, error: &anyhow::Error) {
        self.finished = true;
        self.registry.finish_failed(&self.id, error);
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
/// With a cancel token every command is cancellable.
pub struct DynRunner {
    inner: Box<dyn CommandRunner>,
    cancel: Option<CancelToken>,
}

impl DynRunner {
    pub fn new(inner: Box<dyn CommandRunner>) -> Self {
        Self {
            inner,
            cancel: None,
        }
    }

    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }
}

impl CommandRunner for DynRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        match &self.cancel {
            Some(cancel) => self.inner.run_cancellable(spec, &mut |_| {}, cancel),
            None => self.inner.run(spec),
        }
    }

    fn run_streaming(
        &self,
        spec: &CommandSpec,
        on_line: &mut dyn FnMut(OutputLine),
    ) -> Result<Output> {
        match &self.cancel {
            Some(cancel) => self.inner.run_cancellable(spec, on_line, cancel),
            None => self.inner.run_streaming(spec, on_line),
        }
    }

    fn run_cancellable(
        &self,
        spec: &CommandSpec,
        on_line: &mut dyn FnMut(OutputLine),
        cancel: &CancelToken,
    ) -> Result<Output> {
        self.inner.run_cancellable(spec, on_line, cancel)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus> {
        self.inner.run_interactive(spec)
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
        DynRunner::new((self.0)())
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
