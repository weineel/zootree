use std::collections::HashMap;
use std::process::{Command, Output};
use anyhow::Result;

pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
}

pub trait CommandRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output>;
}

pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let output = cmd.output()?;
        Ok(output)
    }
}

pub struct MockRunner {
    pub calls: std::cell::RefCell<Vec<CommandSpec>>,
    pub responses: std::cell::RefCell<Vec<Output>>,
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
        });
        let output = self.responses.borrow_mut().remove(0);
        Ok(output)
    }
}
