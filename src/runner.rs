use anyhow::Result;
use std::collections::HashMap;
use std::process::{Command, ExitStatus, Output};

pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub env_remove: Vec<String>,
}

pub trait CommandRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output>;

    /// Run a command that needs direct terminal access (interactive TUI).
    /// Inherits stdin/stdout/stderr from the parent process. Returns the
    /// exit status rather than captured output.
    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus>;
}

pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for k in &spec.env_remove {
            cmd.env_remove(k);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let output = cmd.output()?;
        Ok(output)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for k in &spec.env_remove {
            cmd.env_remove(k);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let status = cmd.status()?;
        Ok(status)
    }
}

pub struct MockRunner {
    pub calls: std::cell::RefCell<Vec<CommandSpec>>,
    pub responses: std::cell::RefCell<Vec<Output>>,
}

impl Default for MockRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRunner {
    pub fn new() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            responses: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn push_response(&self, output: Output) {
        self.responses.borrow_mut().push(output);
    }

    pub fn take_calls(&self) -> Vec<CommandSpec> {
        self.calls.borrow_mut().drain(..).collect()
    }
}

impl CommandRunner for MockRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        self.calls.borrow_mut().push(CommandSpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            env_remove: spec.env_remove.clone(),
        });
        let output = self.responses.borrow_mut().remove(0);
        Ok(output)
    }

    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus> {
        self.calls.borrow_mut().push(CommandSpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            env_remove: spec.env_remove.clone(),
        });
        let output = self.responses.borrow_mut().remove(0);
        Ok(output.status)
    }
}
