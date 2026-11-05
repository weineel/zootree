use anyhow::Result;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

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

/// Explicit cancel request for one Operation's commands.
///
/// Cancelling is best effort and one-shot: the request interrupts the command
/// running now (or the next one to start) exactly once, so the rollback a
/// service function runs afterwards is not interrupted again.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<CancelState>);

#[derive(Default)]
struct CancelState {
    requested: AtomicBool,
    pending: AtomicBool,
    interrupted: Mutex<Option<String>>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks for the running command to be interrupted.
    pub fn request(&self) {
        self.0.requested.store(true, Ordering::SeqCst);
        self.0.pending.store(true, Ordering::SeqCst);
    }

    pub fn is_requested(&self) -> bool {
        self.0.requested.load(Ordering::SeqCst)
    }

    /// Consumes the pending request; true when a command must be interrupted.
    pub fn take_pending(&self) -> bool {
        self.0.pending.swap(false, Ordering::SeqCst)
    }

    /// Remembers which command an interrupt hit.
    pub fn record_interrupt(&self, spec: &CommandSpec) {
        let mut text = std::iter::once(spec.program.as_str())
            .chain(spec.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        if text.chars().count() > 200 {
            text = text.chars().take(200).collect::<String>() + "...";
        }
        *self.0.interrupted.lock().unwrap() = Some(text);
    }

    /// The command an interrupt hit, if one was delivered.
    pub fn interrupted_command(&self) -> Option<String> {
        self.0.interrupted.lock().unwrap().clone()
    }

    /// Consumes a pending request for `spec`: records it and returns the error
    /// to hand back instead of running (or finishing) the command.
    pub fn interrupt(&self, spec: &CommandSpec) -> Option<anyhow::Error> {
        if !self.take_pending() {
            return None;
        }
        self.record_interrupt(spec);
        Some(Canceled.into())
    }
}

/// A command was interrupted by an explicit cancel request.
#[derive(Debug)]
pub struct Canceled;

impl std::fmt::Display for Canceled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("interrupted by a cancel request")
    }
}

impl std::error::Error for Canceled {}

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

    /// Like [`run_streaming`](Self::run_streaming), but a cancel request on
    /// `cancel` interrupts the command (best effort) and fails with
    /// [`Canceled`]. The default only honours a request made before the
    /// command starts; [`RealRunner`] also kills a running child.
    fn run_cancellable(
        &self,
        spec: &CommandSpec,
        on_line: &mut dyn FnMut(OutputLine),
        cancel: &CancelToken,
    ) -> Result<Output> {
        if let Some(err) = cancel.interrupt(spec) {
            return Err(err);
        }
        self.run_streaming(spec, on_line)
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
        stream_command(spec, on_line, None)
    }

    fn run_cancellable(
        &self,
        spec: &CommandSpec,
        on_line: &mut dyn FnMut(OutputLine),
        cancel: &CancelToken,
    ) -> Result<Output> {
        stream_command(spec, on_line, Some(cancel))
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

/// Runs `spec`, streaming lines. With a cancel token the child gets its own
/// process group and a watcher kills the whole group on a cancel request.
fn stream_command(
    spec: &CommandSpec,
    on_line: &mut dyn FnMut(OutputLine),
    cancel: Option<&CancelToken>,
) -> Result<Output> {
    if let Some(err) = cancel.and_then(|c| c.interrupt(spec)) {
        return Err(err);
    }
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
    if cancel.is_some() {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let pid = child.id() as libc::pid_t;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let (tx, rx) = mpsc::channel();
    let finished = AtomicBool::new(false);
    let killed = AtomicBool::new(false);
    let (out_bytes, err_bytes) = std::thread::scope(|scope| {
        if let Some(cancel) = cancel {
            scope.spawn(|| {
                while !finished.load(Ordering::SeqCst) {
                    if cancel.take_pending() {
                        cancel.record_interrupt(spec);
                        killed.store(true, Ordering::SeqCst);
                        // SAFETY: plain signal to the child's own process group.
                        unsafe {
                            libc::kill(-pid, libc::SIGKILL);
                        }
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
        }
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
        let joined = (
            out.join().unwrap_or_default(),
            err.join().unwrap_or_default(),
        );
        finished.store(true, Ordering::SeqCst);
        joined
    });
    let status = child.wait()?;
    if killed.load(Ordering::SeqCst) {
        return Err(Canceled.into());
    }
    Ok(Output {
        status,
        stdout: out_bytes,
        stderr: err_bytes,
    })
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
