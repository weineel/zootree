use crate::runner::{CommandRunner, CommandSpec};
use anyhow::Result;
use std::collections::BTreeMap;
use std::process::{ExitStatus, Output};
use std::sync::Mutex;

/// Progress reported by a service function while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceEvent {
    /// The operation began for a Workspace.
    Started {
        operation: &'static str,
        workspace: String,
    },
    /// A user-visible progress line (what the CLI prints to stdout).
    Step(String),
    /// A line of output from a child process.
    Log(String),
    /// A non-fatal problem (what the CLI sends to the log as a warning).
    Warning(String),
    /// The operation ended successfully; `summary` is the closing message.
    Finished {
        operation: &'static str,
        workspace: String,
        summary: String,
    },
}

/// Receives events from a service function. The CLI prints them, the Daemon
/// broadcasts them.
pub trait EventSink {
    fn emit(&self, event: ServiceEvent);
}

/// Sink that records events; used by tests and by callers that replay later.
#[derive(Default)]
pub struct CollectingSink {
    events: Mutex<Vec<ServiceEvent>>,
}

impl CollectingSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<ServiceEvent> {
        self.events.lock().expect("event sink poisoned").clone()
    }
}

impl EventSink for CollectingSink {
    fn emit(&self, event: ServiceEvent) {
        self.events.lock().expect("event sink poisoned").push(event);
    }
}

/// Environment variables that travel with a request.
pub const SNAPSHOT_ENV_VARS: [&str; 7] = [
    "PATH",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "ZELLIJ",
    "ZELLIJ_SESSION_NAME",
    "HERDR_ENV",
    "HERDR_SOCKET_PATH",
];

/// Whitelisted snapshot of the caller's environment. Child processes started
/// by a service function use it instead of the host process's own environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvSnapshot {
    vars: BTreeMap<String, String>,
}

impl EnvSnapshot {
    /// Captures the whitelisted variables from the current process.
    pub fn capture() -> Self {
        Self::from_pairs(
            SNAPSHOT_ENV_VARS
                .iter()
                .filter_map(|name| std::env::var(name).ok().map(|v| ((*name).to_string(), v))),
        )
    }

    /// Builds a snapshot from explicit pairs; names outside the whitelist are dropped.
    pub fn from_pairs<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        Self {
            vars: pairs
                .into_iter()
                .filter(|(name, _)| SNAPSHOT_ENV_VARS.contains(&name.as_str()))
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    pub fn vars(&self) -> &BTreeMap<String, String> {
        &self.vars
    }
}

/// Everything a service function needs besides its request.
pub struct RequestContext<'a> {
    pub events: &'a dyn EventSink,
    pub env: EnvSnapshot,
}

impl<'a> RequestContext<'a> {
    pub fn new(events: &'a dyn EventSink, env: EnvSnapshot) -> Self {
        Self { events, env }
    }

    pub fn emit(&self, event: ServiceEvent) {
        self.events.emit(event);
    }

    pub fn step(&self, message: impl Into<String>) {
        self.emit(ServiceEvent::Step(message.into()));
    }

    pub fn warn(&self, message: impl Into<String>) {
        self.emit(ServiceEvent::Warning(message.into()));
    }
}

/// Runner decorator that applies an [`EnvSnapshot`] to every command: snapshot
/// variables are set (unless the spec already sets them) and whitelisted
/// variables missing from the snapshot are removed.
pub struct EnvRunner<'a, R: CommandRunner> {
    inner: &'a R,
    env: &'a EnvSnapshot,
}

impl<'a, R: CommandRunner> EnvRunner<'a, R> {
    pub fn new(inner: &'a R, env: &'a EnvSnapshot) -> Self {
        Self { inner, env }
    }

    fn apply(&self, spec: &CommandSpec) -> CommandSpec {
        let mut env = spec.env.clone();
        let mut env_remove = spec.env_remove.clone();
        for name in SNAPSHOT_ENV_VARS {
            if spec.env.contains_key(name) {
                continue;
            }
            match self.env.get(name) {
                Some(value) => {
                    env.insert(name.to_string(), value.to_string());
                }
                None => {
                    if !env_remove.iter().any(|n| n == name) {
                        env_remove.push(name.to_string());
                    }
                }
            }
        }
        CommandSpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env,
            env_remove,
        }
    }
}

impl<R: CommandRunner> CommandRunner for EnvRunner<'_, R> {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        self.inner.run(&self.apply(spec))
    }

    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus> {
        self.inner.run_interactive(&self.apply(spec))
    }
}
