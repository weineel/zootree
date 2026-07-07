use super::{LaunchOutcome, MultiplexerIdentity, MultiplexerLaunch, TerminalMultiplexer};
use crate::config::global::MultiplexerKind;
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

pub struct CmuxMultiplexer<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> CmuxMultiplexer<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    fn cmux(&self, args: Vec<String>) -> Result<std::process::Output> {
        self.runner.run(&CommandSpec {
            program: "cmux".into(),
            args,
            cwd: None,
            env: HashMap::new(),
            env_remove: vec![],
        })
    }

    fn ensure_success(output: std::process::Output, context: &str) -> Result<std::process::Output> {
        if !output.status.success() {
            bail!(
                "{} failed: {}",
                context,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(output)
    }

    pub fn parse_workspace_ref(output: &std::process::Output) -> Option<String> {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .flat_map(str::split_whitespace)
            .find(|token| Self::is_workspace_ref(token))
            .map(str::to_string)
    }

    fn is_workspace_ref(token: &str) -> bool {
        let Some(id) = token.strip_prefix("workspace:") else {
            return false;
        };
        !id.is_empty() && id.chars().all(|ch| ch.is_ascii_digit())
    }

    fn parse_unique_workspace_match(stdout: &[u8], display_name: &str) -> Option<String> {
        let stdout = String::from_utf8_lossy(stdout);
        let matches = stdout
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let handle = parts.next()?;
                let rest = parts.collect::<Vec<_>>().join(" ");
                if Self::is_workspace_ref(handle) && rest == display_name {
                    Some(handle.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if matches.len() == 1 {
            Some(matches[0].clone())
        } else {
            None
        }
    }

    fn find_workspace_by_name(&self, display_name: &str) -> Result<Option<String>> {
        let output = self.cmux(vec!["workspace".into(), "list".into()])?;
        let output = Self::ensure_success(output, "cmux workspace list")?;
        Ok(Self::parse_unique_workspace_match(
            &output.stdout,
            display_name,
        ))
    }

    pub fn launch_and_capture_workspace(
        &self,
        launch: &MultiplexerLaunch,
    ) -> Result<Option<String>> {
        let output = self.cmux(vec![
            "workspace".into(),
            "create".into(),
            "--name".into(),
            launch.display_name.clone(),
            "--cwd".into(),
            launch.workspace_dir.to_string_lossy().into_owned(),
            "--layout".into(),
            launch.rendered_layout.clone(),
            "--focus".into(),
            "true".into(),
        ])?;
        let output = Self::ensure_success(output, "cmux workspace create")?;
        Ok(Self::parse_workspace_ref(&output))
    }

    pub fn launch_or_open_and_capture_workspace(
        &self,
        launch: &MultiplexerLaunch,
        identity: &MultiplexerIdentity,
    ) -> Result<Option<String>> {
        if let Some(workspace) = &identity.cmux_workspace {
            let output = self.cmux(vec!["workspace".into(), "select".into(), workspace.clone()])?;
            if output.status.success() {
                return Ok(None);
            }

            tracing::warn!(
                "cmux workspace '{}' could not be selected: {}; recreating workspace",
                workspace,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        self.launch_and_capture_workspace(launch)
    }
}

impl<R: CommandRunner> TerminalMultiplexer for CmuxMultiplexer<'_, R> {
    fn kind(&self) -> MultiplexerKind {
        MultiplexerKind::Cmux
    }

    fn launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome> {
        self.launch_and_capture_workspace(launch)?;
        Ok(LaunchOutcome::Launched)
    }

    fn open(
        &self,
        launch: &MultiplexerLaunch,
        identity: &MultiplexerIdentity,
    ) -> Result<LaunchOutcome> {
        match self.launch_or_open_and_capture_workspace(launch, identity)? {
            Some(_) => Ok(LaunchOutcome::Launched),
            None => Ok(LaunchOutcome::Attached),
        }
    }

    fn close(&self, identity: &MultiplexerIdentity) -> Result<()> {
        let workspace = match &identity.cmux_workspace {
            Some(workspace) => workspace.clone(),
            None => match self.find_workspace_by_name(&identity.display_name)? {
                Some(workspace) => workspace,
                None => {
                    tracing::warn!(
                        "cmux workspace '{}' not found uniquely; skipping cmux close",
                        identity.display_name
                    );
                    return Ok(());
                }
            },
        };
        let output = self.cmux(vec!["workspace".into(), "close".into(), workspace])?;
        Self::ensure_success(output, "cmux workspace close")?;
        Ok(())
    }
}
