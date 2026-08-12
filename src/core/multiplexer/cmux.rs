use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

pub(in crate::core) struct CmuxCommands<'a, R: CommandRunner> {
    runner: &'a R,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) enum FocusResult {
    FocusedExisting,
    FocusedFound(String),
    NotFound,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) enum DeleteResult {
    Deleted { stored_ref_failure: Option<String> },
    NotFound { stored_ref_failure: Option<String> },
    Ambiguous { stored_ref_failure: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CmuxGroupLookup {
    Found(String),
    NotFound,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct RepoWorkspaceSpec {
    pub(in crate::core) repo_name: String,
    pub(in crate::core) workspace_name: String,
    pub(in crate::core) description: String,
    pub(in crate::core) cwd: PathBuf,
    pub(in crate::core) layout: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct GroupSpec {
    pub(in crate::core) group_name: String,
    pub(in crate::core) anchor_name: String,
    pub(in crate::core) anchor_description: String,
    pub(in crate::core) anchor_cwd: PathBuf,
    pub(in crate::core) anchor_layout: String,
    pub(in crate::core) repo_workspaces: Vec<RepoWorkspaceSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct CreatedRepoWorkspace {
    pub(in crate::core) repo: String,
    pub(in crate::core) workspace: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct CreatedGroup {
    pub(in crate::core) group: String,
    pub(in crate::core) repo_workspaces: Vec<CreatedRepoWorkspace>,
}

impl<'a, R: CommandRunner> CmuxCommands<'a, R> {
    pub(in crate::core) fn new(runner: &'a R) -> Self {
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

    fn parse_workspace_ref(output: &std::process::Output) -> Option<String> {
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

    fn parse_workspace_group_ref(output: &std::process::Output) -> Option<String> {
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

    pub(in crate::core) fn focus_group_or_find(
        &self,
        group_name: &str,
        group_ref: Option<&str>,
    ) -> Result<FocusResult> {
        if let Some(group) = group_ref {
            let output = self.cmux(vec!["workspace-group".into(), "focus".into(), group.into()])?;
            if output.status.success() {
                return Ok(FocusResult::FocusedExisting);
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
                return Ok(FocusResult::NotFound);
            }
            CmuxGroupLookup::Ambiguous => {
                tracing::debug!("cmux group '{}' is ambiguous; skipping focus", group_name);
                return Ok(FocusResult::Ambiguous);
            }
        };
        self.focus_group_ref(&group)?;
        Ok(FocusResult::FocusedFound(group))
    }

    pub(in crate::core) fn delete_group(
        &self,
        group_name: &str,
        group_ref: Option<&str>,
    ) -> Result<DeleteResult> {
        let mut stored_ref_failure = None;
        if let Some(group) = group_ref {
            let output = self.cmux(vec![
                "workspace-group".into(),
                "delete".into(),
                group.into(),
            ])?;
            if output.status.success() {
                return Ok(DeleteResult::Deleted {
                    stored_ref_failure: None,
                });
            }
            let failure = format!(
                "stored cmux group '{}' could not be deleted: {}",
                group,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            tracing::warn!(
                "cmux group '{}' could not be deleted: {}; trying title lookup",
                group,
                String::from_utf8_lossy(&output.stderr)
            );
            stored_ref_failure = Some(failure);
        }

        let group = match self.find_group_by_name(group_name)? {
            CmuxGroupLookup::Found(group) => group,
            CmuxGroupLookup::NotFound => {
                tracing::warn!(
                    "cmux group '{}' not found; skipping cmux group delete",
                    group_name
                );
                return Ok(DeleteResult::NotFound { stored_ref_failure });
            }
            CmuxGroupLookup::Ambiguous => {
                tracing::warn!(
                    "cmux group '{}' is ambiguous; skipping cmux group delete",
                    group_name
                );
                return Ok(DeleteResult::Ambiguous { stored_ref_failure });
            }
        };

        let output = self.cmux(vec!["workspace-group".into(), "delete".into(), group])?;
        Self::ensure_success(output, "cmux workspace-group delete")?;
        Ok(DeleteResult::Deleted { stored_ref_failure })
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

    fn rollback_group_creation(&self, group: Option<&str>, workspaces: &[String]) {
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

    pub(in crate::core) fn create_group_environment(
        &self,
        spec: &GroupSpec,
    ) -> Result<CreatedGroup> {
        let Some(first_repo) = spec.repo_workspaces.first() else {
            anyhow::bail!("cmux group creation requires at least one repo workspace");
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
        let group = match self.create_group(&spec.group_name, &first_repo_workspace) {
            Ok(group) => group,
            Err(err) => {
                self.rollback_group_creation(None, &created_workspaces);
                return Err(err);
            }
        };
        let generated_anchor = match self.group_anchor_workspace(&group) {
            Ok(workspace) => workspace,
            Err(err) => {
                self.rollback_group_creation(Some(&group), &created_workspaces);
                return Err(err);
            }
        };
        let anchor_workspace = match self.create_workspace(
            &spec.anchor_name,
            &spec.anchor_description,
            &spec.anchor_cwd,
            &spec.anchor_layout,
            true,
            Some((&group, "top")),
        ) {
            Ok(workspace) => workspace,
            Err(err) => {
                self.rollback_group_creation(Some(&group), &created_workspaces);
                return Err(err);
            }
        };
        created_workspaces.push(anchor_workspace.clone());
        if let Err(err) = self.set_group_anchor(&group, &anchor_workspace) {
            self.rollback_group_creation(Some(&group), &created_workspaces);
            return Err(err);
        }
        if let Err(err) = self.close_workspace_ref(&generated_anchor) {
            self.rollback_group_creation(Some(&group), &created_workspaces);
            return Err(err);
        }

        let mut repo_workspaces = vec![CreatedRepoWorkspace {
            repo: first_repo.repo_name.clone(),
            workspace: first_repo_workspace,
        }];
        for repo in spec.repo_workspaces.iter().skip(1) {
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
                    self.rollback_group_creation(Some(&group), &created_workspaces);
                    return Err(err);
                }
            };
            created_workspaces.push(workspace.clone());
            repo_workspaces.push(CreatedRepoWorkspace {
                repo: repo.repo_name.clone(),
                workspace,
            });
        }

        Ok(CreatedGroup {
            group,
            repo_workspaces,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    fn output(status: i32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(status << 8),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    fn group_spec() -> GroupSpec {
        GroupSpec {
            group_name: "Fix cmux sidebar copy".into(),
            anchor_name: "zootree-fair-fox".into(),
            anchor_description: "Fix cmux sidebar copy".into(),
            anchor_cwd: "/tmp/fair-fox".into(),
            anchor_layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"info"}]}}"#.into(),
            repo_workspaces: vec![
                RepoWorkspaceSpec {
                    repo_name: "api".into(),
                    workspace_name: "zootree-fair-fox-api".into(),
                    description: "api".into(),
                    cwd: "/tmp/fair-fox/api".into(),
                    layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"api"}]}}"#.into(),
                },
                RepoWorkspaceSpec {
                    repo_name: "web".into(),
                    workspace_name: "zootree-fair-fox-web".into(),
                    description: "web".into(),
                    cwd: "/tmp/fair-fox/web".into(),
                    layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"web"}]}}"#.into(),
                },
            ],
        }
    }

    #[test]
    fn parsers_accept_only_numeric_cmux_refs() {
        let workspace = output(
            0,
            b"workspace:bogus\nworkspace:9,\nworkspace:10 created\n",
            b"",
        );
        let group = output(
            0,
            b"workspace_group:bogus\ncreated workspace_group:3\n",
            b"",
        );

        assert_eq!(
            CmuxCommands::<MockRunner>::parse_workspace_ref(&workspace).as_deref(),
            Some("workspace:10")
        );
        assert_eq!(
            CmuxCommands::<MockRunner>::parse_workspace_group_ref(&group).as_deref(),
            Some("workspace_group:3")
        );
    }

    #[test]
    fn group_lookup_preserves_unique_and_ambiguous_results() {
        let unique = br#"{"groups":[
            {"ref":"workspace_group:2","name":"Fix cmux sidebar copy"},
            {"ref":"workspace_group:3","name":"Other work"}
        ]}"#;
        let duplicate = br#"{"groups":[
            {"ref":"workspace_group:2","name":"Fix cmux sidebar copy"},
            {"ref":"workspace_group:3","name":"Fix cmux sidebar copy"}
        ]}"#;

        assert_eq!(
            CmuxCommands::<MockRunner>::parse_group_lookup(unique, "Fix cmux sidebar copy"),
            CmuxGroupLookup::Found("workspace_group:2".into())
        );
        assert_eq!(
            CmuxCommands::<MockRunner>::parse_group_lookup(duplicate, "Fix cmux sidebar copy"),
            CmuxGroupLookup::Ambiguous
        );
    }

    #[test]
    fn focus_uses_stored_group_ref_without_lookup() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"", b""));

        let result = CmuxCommands::new(&runner)
            .focus_group_or_find("Fix cmux sidebar copy", Some("workspace_group:2"))
            .unwrap();

        assert_eq!(result, FocusResult::FocusedExisting);
        assert_eq!(
            runner.take_calls()[0].args,
            vec!["workspace-group", "focus", "workspace_group:2"]
        );
    }

    #[test]
    fn focus_falls_back_from_stale_ref_to_unique_name() {
        let runner = MockRunner::new();
        runner.push_response(output(1, b"", b"group not found"));
        runner.push_response(output(
            0,
            br#"{"groups":[{"ref":"workspace_group:7","name":"Fix cmux sidebar copy"}]}"#,
            b"",
        ));
        runner.push_response(output(0, b"", b""));

        let result = CmuxCommands::new(&runner)
            .focus_group_or_find("Fix cmux sidebar copy", Some("workspace_group:2"))
            .unwrap();

        assert_eq!(
            result,
            FocusResult::FocusedFound("workspace_group:7".into())
        );
        let calls = runner.take_calls();
        assert_eq!(calls[1].args, vec!["workspace-group", "list", "--json"]);
        assert_eq!(
            calls[2].args,
            vec!["workspace-group", "focus", "workspace_group:7"]
        );
    }

    #[test]
    fn delete_falls_back_from_stale_ref_to_unique_name() {
        let runner = MockRunner::new();
        runner.push_response(output(1, b"", b"group not found"));
        runner.push_response(output(
            0,
            br#"{"groups":[{"ref":"workspace_group:7","name":"Fix cmux sidebar copy"}]}"#,
            b"",
        ));
        runner.push_response(output(0, b"", b""));

        let result = CmuxCommands::new(&runner)
            .delete_group("Fix cmux sidebar copy", Some("workspace_group:2"))
            .unwrap();

        assert_eq!(
            result,
            DeleteResult::Deleted {
                stored_ref_failure: Some(
                    "stored cmux group 'workspace_group:2' could not be deleted: group not found"
                        .into()
                )
            }
        );
        assert_eq!(
            runner.take_calls()[2].args,
            vec!["workspace-group", "delete", "workspace_group:7"]
        );
    }

    #[test]
    fn create_group_environment_translates_full_command_sequence() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"workspace:4\n", b""));
        runner.push_response(output(0, b"workspace_group:2\n", b""));
        runner.push_response(output(
            0,
            br#"{"groups":[{"ref":"workspace_group:2","anchor_workspace_ref":"workspace:99"}]}"#,
            b"",
        ));
        runner.push_response(output(0, b"workspace:7\n", b""));
        runner.push_response(output(0, b"", b""));
        runner.push_response(output(0, b"", b""));
        runner.push_response(output(0, b"workspace:5\n", b""));

        let created = CmuxCommands::new(&runner)
            .create_group_environment(&group_spec())
            .unwrap();

        assert_eq!(created.group, "workspace_group:2");
        assert_eq!(
            created.repo_workspaces,
            vec![
                CreatedRepoWorkspace {
                    repo: "api".into(),
                    workspace: "workspace:4".into()
                },
                CreatedRepoWorkspace {
                    repo: "web".into(),
                    workspace: "workspace:5".into()
                }
            ]
        );
        let calls = runner.take_calls();
        assert_eq!(calls.len(), 7);
        assert_eq!(
            calls[0].args,
            vec![
                "workspace",
                "create",
                "--name",
                "zootree-fair-fox-api",
                "--description",
                "api",
                "--cwd",
                "/tmp/fair-fox/api",
                "--layout",
                r#"{"pane":{"surfaces":[{"type":"terminal","name":"api"}]}}"#,
                "--focus",
                "true"
            ]
        );
        assert_eq!(
            calls[1].args,
            vec![
                "workspace-group",
                "create",
                "--name",
                "Fix cmux sidebar copy",
                "--from",
                "workspace:4"
            ]
        );
        assert_eq!(calls[2].args, vec!["workspace-group", "list", "--json"]);
        assert_eq!(
            calls[4].args,
            vec![
                "workspace-group",
                "set-anchor",
                "--group",
                "workspace_group:2",
                "--workspace",
                "workspace:7"
            ]
        );
        assert_eq!(calls[5].args, vec!["workspace", "close", "workspace:99"]);
        assert_eq!(
            &calls[6].args[calls[6].args.len() - 4..],
            ["--group", "workspace_group:2", "--group-placement", "end"]
        );
    }

    #[test]
    fn create_group_environment_rolls_back_first_workspace_when_group_ref_is_missing() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"workspace:4\n", b""));
        runner.push_response(output(0, b"created group without ref\n", b""));
        runner.push_response(output(0, b"", b""));

        let error = CmuxCommands::new(&runner)
            .create_group_environment(&group_spec())
            .unwrap_err();

        assert!(error.to_string().contains("did not return a group ref"));
        assert_eq!(
            runner.take_calls()[2].args,
            vec!["workspace", "close", "workspace:4"]
        );
    }

    #[test]
    fn create_group_environment_rolls_back_group_when_later_repo_fails() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"workspace:4\n", b""));
        runner.push_response(output(0, b"workspace_group:2\n", b""));
        runner.push_response(output(
            0,
            br#"{"groups":[{"ref":"workspace_group:2","anchor_workspace_ref":"workspace:99"}]}"#,
            b"",
        ));
        runner.push_response(output(0, b"workspace:7\n", b""));
        runner.push_response(output(0, b"", b""));
        runner.push_response(output(0, b"", b""));
        runner.push_response(output(1, b"", b"second repo create failed"));
        runner.push_response(output(0, b"", b""));

        let error = CmuxCommands::new(&runner)
            .create_group_environment(&group_spec())
            .unwrap_err();

        assert!(error.to_string().contains("second repo create failed"));
        assert_eq!(
            runner.take_calls()[7].args,
            vec!["workspace-group", "delete", "workspace_group:2"]
        );
    }
}
