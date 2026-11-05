use anyhow::Result;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;

pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub env_remove: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// One line of child output, without its trailing newline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub stream: OutputStream,
    pub text: String,
}

pub trait CommandRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Output>;

    /// Like [`run`](Self::run), but also hands every output line to `on_line`
    /// while the command runs. The full output is still captured and returned,
    /// including when the command fails.
    ///
    /// The default implementation runs to completion first and then replays
    /// stdout lines followed by stderr lines; runners that can do better
    /// (see [`RealRunner`]) override it.
    fn run_streaming(
        &self,
        spec: &CommandSpec,
        on_line: &mut dyn FnMut(OutputLine),
    ) -> Result<Output> {
        let output = self.run(spec)?;
        replay_lines(&output, on_line);
        Ok(output)
    }

    /// Run a command that needs direct terminal access (interactive TUI).
    /// Inherits stdin/stdout/stderr from the parent process. Returns the
    /// exit status rather than captured output.
    fn run_interactive(&self, spec: &CommandSpec) -> Result<ExitStatus>;
}

/// Feeds the captured output of a finished command to `on_line`.
pub fn replay_lines(output: &Output, on_line: &mut dyn FnMut(OutputLine)) {
    for (stream, bytes) in [
        (OutputStream::Stdout, &output.stdout),
        (OutputStream::Stderr, &output.stderr),
    ] {
        for line in String::from_utf8_lossy(bytes).lines() {
            on_line(OutputLine {
                stream,
                text: line.to_string(),
            });
        }
    }
}

fn read_lines<T: Read>(
    reader: T,
    stream: OutputStream,
    tx: mpsc::Sender<(OutputStream, Vec<u8>)>,
) -> Vec<u8> {
    let mut reader = BufReader::new(reader);
    let mut all = Vec::new();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                all.extend_from_slice(&buf);
                let _ = tx.send((stream, buf.clone()));
            }
        }
    }
    all
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

    fn run_streaming(
        &self,
        spec: &CommandSpec,
        on_line: &mut dyn FnMut(OutputLine),
    ) -> Result<Output> {
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
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let (tx, rx) = mpsc::channel();
        let (out_bytes, err_bytes) = std::thread::scope(|scope| {
            let out_tx = tx.clone();
            let out = scope.spawn(move || read_lines(stdout, OutputStream::Stdout, out_tx));
            let err = scope.spawn(move || read_lines(stderr, OutputStream::Stderr, tx));
            // The channel closes once both reader threads finish.
            for (stream, bytes) in rx {
                let text = String::from_utf8_lossy(&bytes);
                on_line(OutputLine {
                    stream,
                    text: text.trim_end_matches(['\n', '\r']).to_string(),
                });
            }
            (
                out.join().unwrap_or_default(),
                err.join().unwrap_or_default(),
            )
        });
        let status = child.wait()?;
        Ok(Output {
            status,
            stdout: out_bytes,
            stderr: err_bytes,
        })
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
