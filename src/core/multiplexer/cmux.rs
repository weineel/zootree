use super::{
    CmuxCapturedGroupState, CmuxGroupLaunch, LaunchOutcome, MultiplexerIdentity, MultiplexerLaunch,
    TerminalMultiplexer,
};
use crate::config::global::MultiplexerKind;
use crate::config::workspace::CmuxRepoWorkspaceState;
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::HashMap;

pub struct CmuxMultiplexer<'a, R: CommandRunner> {
    runner: &'a R,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmuxGroupFocusOutcome {
    FocusedExisting,
    FocusedFound(String),
    NotFound,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CmuxGroupLookup {
    Found(String),
    NotFound,
    Ambiguous,
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

    pub fn parse_workspace_group_ref(output: &std::process::Output) -> Option<String> {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .flat_map(str::split_whitespace)
            .find(|token| Self::is_workspace_group_ref(token))
            .map(str::to_string)
    }

    fn is_workspace_group_ref(token: &str) -> bool {
        let Some(id) = token.strip_prefix("workspace_group:") else {
            return false;
        };
        !id.is_empty() && id.chars().all(|ch| ch.is_ascii_digit())
    }

    pub fn parse_unique_group_match(stdout: &[u8], group_name: &str) -> Option<String> {
        match Self::parse_group_lookup(stdout, group_name) {
            CmuxGroupLookup::Found(group) => Some(group),
            CmuxGroupLookup::NotFound | CmuxGroupLookup::Ambiguous => None,
        }
    }

    fn parse_group_lookup(stdout: &[u8], group_name: &str) -> CmuxGroupLookup {
        let Some(value) = serde_json::from_slice::<Value>(stdout).ok() else {
            return CmuxGroupLookup::NotFound;
        };
        let Some(groups) = value.get("groups").and_then(Value::as_array) else {
            return CmuxGroupLookup::NotFound;
        };
        let matches = groups
            .iter()
            .filter(|group| {
                let name = group
                    .get("name")
                    .or_else(|| group.get("title"))
                    .and_then(Value::as_str);
                name == Some(group_name)
            })
            .collect::<Vec<_>>();

        match matches.len() {
            0 => return CmuxGroupLookup::NotFound,
            1 => {}
            _ => return CmuxGroupLookup::Ambiguous,
        }

        matches[0]
            .get("ref")
            .or_else(|| matches[0].get("workspace_group"))
            .or_else(|| matches[0].get("id"))
            .and_then(Value::as_str)
            .filter(|value| Self::is_workspace_group_ref(value))
            .map(str::to_string)
            .map(CmuxGroupLookup::Found)
            .unwrap_or(CmuxGroupLookup::NotFound)
    }

    fn parse_group_anchor_ref(stdout: &[u8], group_ref: &str) -> Option<String> {
        let value = serde_json::from_slice::<Value>(stdout).ok()?;
        let groups = value.get("groups").and_then(Value::as_array)?;
        groups
            .iter()
            .find(|group| {
                group
                    .get("ref")
                    .or_else(|| group.get("workspace_group"))
                    .or_else(|| group.get("id"))
                    .and_then(Value::as_str)
                    == Some(group_ref)
            })
            .and_then(|group| group.get("anchor_workspace_ref"))
            .and_then(Value::as_str)
            .filter(|value| Self::is_workspace_ref(value))
            .map(str::to_string)
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

    fn find_group_by_name(&self, group_name: &str) -> Result<CmuxGroupLookup> {
        let output = self.cmux(vec![
            "workspace-group".into(),
            "list".into(),
            "--json".into(),
        ])?;
        let output = Self::ensure_success(output, "cmux workspace-group list")?;
        Ok(Self::parse_group_lookup(&output.stdout, group_name))
    }

    fn focus_group_ref(&self, group: &str) -> Result<()> {
        let output = self.cmux(vec!["workspace-group".into(), "focus".into(), group.into()])?;
        Self::ensure_success(output, "cmux workspace-group focus")?;
        Ok(())
    }

    pub fn focus_group_or_find(
        &self,
        group_name: &str,
        group_ref: Option<&str>,
    ) -> Result<CmuxGroupFocusOutcome> {
        if let Some(group) = group_ref {
            let output = self.cmux(vec!["workspace-group".into(), "focus".into(), group.into()])?;
            if output.status.success() {
                return Ok(CmuxGroupFocusOutcome::FocusedExisting);
            }
            tracing::debug!(
                "cmux group '{}' could not be focused: {}; trying title lookup",
                group,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let group = match self.find_group_by_name(group_name)? {
            CmuxGroupLookup::Found(group) => group,
            CmuxGroupLookup::NotFound => {
                tracing::debug!("cmux group '{}' not found; skipping focus", group_name);
                return Ok(CmuxGroupFocusOutcome::NotFound);
            }
            CmuxGroupLookup::Ambiguous => {
                tracing::debug!("cmux group '{}' is ambiguous; skipping focus", group_name);
                return Ok(CmuxGroupFocusOutcome::Ambiguous);
            }
        };
        self.focus_group_ref(&group)?;
        Ok(CmuxGroupFocusOutcome::FocusedFound(group))
    }

    pub fn delete_group(&self, group_name: &str, group_ref: Option<&str>) -> Result<()> {
        if let Some(group) = group_ref {
            let output = self.cmux(vec![
                "workspace-group".into(),
                "delete".into(),
                group.into(),
            ])?;
            if output.status.success() {
                return Ok(());
            }
            tracing::warn!(
                "cmux group '{}' could not be deleted: {}; trying title lookup",
                group,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let group = match self.find_group_by_name(group_name)? {
            CmuxGroupLookup::Found(group) => group,
            CmuxGroupLookup::NotFound => {
                tracing::warn!(
                    "cmux group '{}' not found; skipping cmux group delete",
                    group_name
                );
                return Ok(());
            }
            CmuxGroupLookup::Ambiguous => {
                tracing::warn!(
                    "cmux group '{}' is ambiguous; skipping cmux group delete",
                    group_name
                );
                return Ok(());
            }
        };

        let output = self.cmux(vec!["workspace-group".into(), "delete".into(), group])?;
        Self::ensure_success(output, "cmux workspace-group delete")?;
        Ok(())
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
            "--description".into(),
            launch.description.clone(),
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

    fn create_workspace(
        &self,
        name: &str,
        description: &str,
        cwd: &std::path::Path,
        layout: &str,
        focus: bool,
        group: Option<(&str, &str)>,
    ) -> Result<String> {
        let mut args = vec![
            "workspace".into(),
            "create".into(),
            "--name".into(),
            name.into(),
            "--description".into(),
            description.into(),
            "--cwd".into(),
            cwd.to_string_lossy().into_owned(),
            "--layout".into(),
            layout.into(),
            "--focus".into(),
            focus.to_string(),
        ];
        if let Some((group, placement)) = group {
            args.extend([
                "--group".into(),
                group.into(),
                "--group-placement".into(),
                placement.into(),
            ]);
        }
        let output = self.cmux(args)?;
        let output = Self::ensure_success(output, "cmux workspace create")?;
        Self::parse_workspace_ref(&output).ok_or_else(|| {
            anyhow::anyhow!(
                "cmux workspace create for '{}' did not return a workspace ref",
                name
            )
        })
    }

    fn create_group(&self, name: &str, anchor_workspace: &str) -> Result<String> {
        let output = self.cmux(vec![
            "workspace-group".into(),
            "create".into(),
            "--name".into(),
            name.into(),
            "--from".into(),
            anchor_workspace.into(),
        ])?;
        let output = Self::ensure_success(output, "cmux workspace-group create")?;
        Self::parse_workspace_group_ref(&output).ok_or_else(|| {
            anyhow::anyhow!(
                "cmux workspace-group create for '{}' did not return a group ref",
                name
            )
        })
    }

    fn delete_group_ref(&self, group: &str) -> Result<()> {
        let output = self.cmux(vec![
            "workspace-group".into(),
            "delete".into(),
            group.into(),
        ])?;
        Self::ensure_success(output, "cmux workspace-group delete")?;
        Ok(())
    }

    fn group_anchor_workspace(&self, group: &str) -> Result<String> {
        let output = self.cmux(vec![
            "workspace-group".into(),
            "list".into(),
            "--json".into(),
        ])?;
        let output = Self::ensure_success(output, "cmux workspace-group list")?;
        Self::parse_group_anchor_ref(&output.stdout, group).ok_or_else(|| {
            anyhow::anyhow!(
                "cmux workspace-group list did not include anchor for '{}'",
                group
            )
        })
    }

    fn set_group_anchor(&self, group: &str, workspace: &str) -> Result<()> {
        let output = self.cmux(vec![
            "workspace-group".into(),
            "set-anchor".into(),
            "--group".into(),
            group.into(),
            "--workspace".into(),
            workspace.into(),
        ])?;
        Self::ensure_success(output, "cmux workspace-group set-anchor")?;
        Ok(())
    }

    fn close_workspace_ref(&self, workspace: &str) -> Result<()> {
        let output = self.cmux(vec!["workspace".into(), "close".into(), workspace.into()])?;
        Self::ensure_success(output, "cmux workspace close")?;
        Ok(())
    }

    fn rollback_group_launch(&self, group: Option<&str>, workspaces: &[String]) {
        if let Some(group) = group {
            match self.delete_group_ref(group) {
                Ok(()) => return,
                Err(err) => {
                    tracing::warn!("failed to rollback cmux group '{}': {}", group, err);
                }
            }
        }

        for workspace in workspaces.iter().rev() {
            if let Err(err) = self.close_workspace_ref(workspace) {
                tracing::warn!("failed to rollback cmux workspace '{}': {}", workspace, err);
            }
        }
    }

    pub fn launch_group_and_capture_state(
        &self,
        launch: &CmuxGroupLaunch,
    ) -> Result<CmuxCapturedGroupState> {
        let Some(first_repo) = launch.repo_workspaces.first() else {
            anyhow::bail!("cmux group launch requires at least one repo workspace");
        };
        let mut created_workspaces = Vec::new();
        let first_repo_workspace = match self.create_workspace(
            &first_repo.workspace_name,
            &first_repo.description,
            &first_repo.cwd,
            &first_repo.layout,
            true,
            None,
        ) {
            Ok(workspace) => workspace,
            Err(err) => {
                return Err(err);
            }
        };
        created_workspaces.push(first_repo_workspace.clone());
        let group = match self.create_group(&launch.group_name, &first_repo_workspace) {
            Ok(group) => group,
            Err(err) => {
                self.rollback_group_launch(None, &created_workspaces);
                return Err(err);
            }
        };
        let generated_anchor = match self.group_anchor_workspace(&group) {
            Ok(workspace) => workspace,
            Err(err) => {
                self.rollback_group_launch(Some(&group), &created_workspaces);
                return Err(err);
            }
        };
        let anchor_workspace = match self.create_workspace(
            &launch.anchor_name,
            &launch.anchor_description,
            &launch.anchor_cwd,
            &launch.anchor_layout,
            true,
            Some((&group, "top")),
        ) {
            Ok(workspace) => workspace,
            Err(err) => {
                self.rollback_group_launch(Some(&group), &created_workspaces);
                return Err(err);
            }
        };
        created_workspaces.push(anchor_workspace.clone());
        if let Err(err) = self.set_group_anchor(&group, &anchor_workspace) {
            self.rollback_group_launch(Some(&group), &created_workspaces);
            return Err(err);
        }
        if let Err(err) = self.close_workspace_ref(&generated_anchor) {
            self.rollback_group_launch(Some(&group), &created_workspaces);
            return Err(err);
        }

        let mut repo_workspaces = vec![CmuxRepoWorkspaceState {
            repo: first_repo.repo_name.clone(),
            workspace: first_repo_workspace,
        }];
        for repo in launch.repo_workspaces.iter().skip(1) {
            let workspace = match self.create_workspace(
                &repo.workspace_name,
                &repo.description,
                &repo.cwd,
                &repo.layout,
                false,
                Some((&group, "end")),
            ) {
                Ok(workspace) => workspace,
                Err(err) => {
                    self.rollback_group_launch(Some(&group), &created_workspaces);
                    return Err(err);
                }
            };
            created_workspaces.push(workspace.clone());
            repo_workspaces.push(CmuxRepoWorkspaceState {
                repo: repo.repo_name.clone(),
                workspace,
            });
        }

        Ok(CmuxCapturedGroupState {
            group,
            repo_workspaces,
        })
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
