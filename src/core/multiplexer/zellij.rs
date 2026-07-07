use super::{LaunchOutcome, MultiplexerIdentity, MultiplexerLaunch, TerminalMultiplexer};
use crate::config::global::MultiplexerKind;
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

pub fn is_inside_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some() || std::env::var_os("ZELLIJ_SESSION_NAME").is_some()
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchPlan {
    ForegroundCreate,
    ForegroundAttach,
    BackgroundCreate,
    AlreadyRunningHint,
}

pub fn plan_launch(in_zellij: bool, session_exists: bool) -> LaunchPlan {
    match (in_zellij, session_exists) {
        (false, false) => LaunchPlan::ForegroundCreate,
        (false, true) => LaunchPlan::ForegroundAttach,
        (true, false) => LaunchPlan::BackgroundCreate,
        (true, true) => LaunchPlan::AlreadyRunningHint,
    }
}

fn session_list_line_matches(line: &str, session_name: &str) -> bool {
    let line = strip_ansi_escape_sequences(line);
    line.split_whitespace().next() == Some(session_name)
}

fn strip_ansi_escape_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

pub struct ZellijMultiplexer<'a, R: CommandRunner> {
    runner: &'a R,
    in_zellij: bool,
}

impl<'a, R: CommandRunner> ZellijMultiplexer<'a, R> {
    pub fn new(runner: &'a R, in_zellij: bool) -> Self {
        Self { runner, in_zellij }
    }

    fn zellij(&self, args: Vec<String>) -> Result<std::process::Output> {
        self.runner.run(&CommandSpec {
            program: "zellij".into(),
            args,
            cwd: None,
            env: HashMap::new(),
            env_remove: vec![],
        })
    }

    fn zellij_interactive(&self, args: Vec<String>) -> Result<()> {
        let status = self.runner.run_interactive(&CommandSpec {
            program: "zellij".into(),
            args,
            cwd: None,
            env: HashMap::new(),
            env_remove: vec![],
        })?;
        if !status.success() {
            let reason = status
                .code()
                .map(|c| format!("exit code {}", c))
                .unwrap_or_else(|| "terminated by signal".into());
            bail!("zellij exited with {}", reason);
        }
        Ok(())
    }

    fn start_session(&self, session_name: &str, layout_path: &Path) -> Result<()> {
        info!("starting zellij session: {}", session_name);
        self.zellij_interactive(vec![
            "--new-session-with-layout".into(),
            layout_path.to_string_lossy().into(),
            "--session".into(),
            session_name.into(),
        ])
    }

    fn start_session_background(&self, session_name: &str, layout_path: &Path) -> Result<()> {
        info!("starting zellij session in background: {}", session_name);
        let output = self.runner.run(&CommandSpec {
            program: "zellij".into(),
            args: vec![
                "-l".into(),
                layout_path.to_string_lossy().into(),
                "attach".into(),
                "--create-background".into(),
                session_name.into(),
            ],
            cwd: None,
            env: HashMap::new(),
            env_remove: vec![
                "ZELLIJ".into(),
                "ZELLIJ_SESSION_NAME".into(),
                "ZELLIJ_PANE_ID".into(),
            ],
        })?;
        if !output.status.success() {
            bail!(
                "zellij background session create failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn attach_session(&self, session_name: &str) -> Result<()> {
        info!("attaching to zellij session: {}", session_name);
        self.zellij_interactive(vec!["attach".into(), session_name.into()])
    }

    fn session_exists(&self, session_name: &str) -> Result<bool> {
        let output = self.zellij(vec!["list-sessions".into()])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .any(|line| session_list_line_matches(line, session_name)))
    }

    fn run_launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome> {
        let session_exists = self.session_exists(&launch.display_name)?;
        let plan = plan_launch(self.in_zellij, session_exists);
        match plan {
            LaunchPlan::ForegroundCreate => {
                self.start_session(&launch.display_name, &launch.layout_file)?;
                Ok(LaunchOutcome::Launched)
            }
            LaunchPlan::ForegroundAttach => {
                self.attach_session(&launch.display_name)?;
                Ok(LaunchOutcome::Attached)
            }
            LaunchPlan::BackgroundCreate => {
                self.start_session_background(&launch.display_name, &launch.layout_file)?;
                println!(
                    "zellij session '{}' is running in background.",
                    launch.display_name
                );
                println!(
                    "Run `zootree open {}` (outside zellij) to attach.",
                    launch.workspace_name
                );
                Ok(LaunchOutcome::BackgroundCreated)
            }
            LaunchPlan::AlreadyRunningHint => {
                println!("zellij session '{}' already exists.", launch.display_name);
                println!(
                    "Run `zootree open {}` (outside zellij) to attach.",
                    launch.workspace_name
                );
                Ok(LaunchOutcome::AlreadyRunning)
            }
        }
    }
}

impl<R: CommandRunner> TerminalMultiplexer for ZellijMultiplexer<'_, R> {
    fn kind(&self) -> MultiplexerKind {
        MultiplexerKind::Zellij
    }

    fn launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome> {
        self.run_launch(launch)
    }

    fn open(
        &self,
        launch: &MultiplexerLaunch,
        _identity: &MultiplexerIdentity,
    ) -> Result<LaunchOutcome> {
        self.run_launch(launch)
    }

    fn close(&self, identity: &MultiplexerIdentity) -> Result<()> {
        info!("killing zellij session: {}", identity.display_name);
        let _ = self.zellij(vec![
            "delete-session".into(),
            "--force".into(),
            identity.display_name.clone(),
        ]);
        Ok(())
    }
}
