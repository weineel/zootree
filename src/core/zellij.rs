use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

pub struct ZellijOps<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> ZellijOps<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    fn zellij(&self, args: Vec<String>) -> Result<std::process::Output> {
        let spec = CommandSpec {
            program: "zellij".into(),
            args,
            cwd: None,
            env: HashMap::new(),
        };
        self.runner.run(&spec)
    }

    fn zellij_interactive(&self, args: Vec<String>) -> Result<()> {
        let spec = CommandSpec {
            program: "zellij".into(),
            args,
            cwd: None,
            env: HashMap::new(),
        };
        let status = self.runner.run_interactive(&spec)?;
        if !status.success() {
            let reason = status
                .code()
                .map(|c| format!("exit code {}", c))
                .unwrap_or_else(|| "terminated by signal".into());
            bail!("zellij exited with {}", reason);
        }
        Ok(())
    }

    pub fn start_session(&self, session_name: &str, layout_path: &Path) -> Result<()> {
        info!("starting zellij session: {}", session_name);
        self.zellij_interactive(vec![
            "--new-session-with-layout".into(),
            layout_path.to_string_lossy().into(),
            "--session".into(),
            session_name.into(),
        ])
    }

    pub fn attach_session(&self, session_name: &str) -> Result<()> {
        info!("attaching to zellij session: {}", session_name);
        self.zellij_interactive(vec!["attach".into(), session_name.into()])
    }

    pub fn kill_session(&self, session_name: &str) -> Result<()> {
        info!("killing zellij session: {}", session_name);
        let _ = self.zellij(vec![
            "delete-session".into(),
            "--force".into(),
            session_name.into(),
        ]);
        Ok(())
    }

    pub fn session_exists(&self, session_name: &str) -> Result<bool> {
        let output = self.zellij(vec!["list-sessions".into()])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .any(|line| line.trim().starts_with(session_name)))
    }

    pub fn add_tab(&self, session_name: &str, layout_path: &Path, tab_name: &str) -> Result<()> {
        info!("adding tab '{}' to session '{}'", tab_name, session_name);
        self.zellij(vec![
            "--session".into(),
            session_name.into(),
            "action".into(),
            "new-tab".into(),
            "--layout".into(),
            layout_path.to_string_lossy().into(),
            "--name".into(),
            tab_name.into(),
        ])?;
        Ok(())
    }
}
