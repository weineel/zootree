use crate::config::global::HookValue;
use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

const OFFICIAL_HOOK_ENV_VARS: [&str; 15] = [
    "ZOOTREE_HOOK",
    "ZOOTREE_OPERATION",
    "ZOOTREE_HOOK_SCOPE",
    "ZOOTREE_HOOK_CONFIG_SCOPE",
    "ZOOTREE_WORKSPACE",
    "ZOOTREE_WORKSPACE_TITLE",
    "ZOOTREE_WORKSPACE_DESCRIPTION",
    "ZOOTREE_WORKSPACE_STATUS",
    "ZOOTREE_WORKSPACE_DIR",
    "ZOOTREE_BRANCH",
    "ZOOTREE_VERSION",
    "ZOOTREE_REPO",
    "ZOOTREE_REPO_SOURCE_DIR",
    "ZOOTREE_WORKTREE_PATH",
    "ZOOTREE_TARGET_BRANCH",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookStage {
    PostCreate,
    PostStart,
    PreDone,
    PreCancel,
    PreRemove,
}

impl HookStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::PostCreate => "post_create",
            Self::PostStart => "post_start",
            Self::PreDone => "pre_done",
            Self::PreCancel => "pre_cancel",
            Self::PreRemove => "pre_remove",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOperation {
    Start,
    Reopen,
    AddRepo,
    Done,
    Cancel,
}

impl HookOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Reopen => "reopen",
            Self::AddRepo => "add-repo",
            Self::Done => "done",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookConfigScope {
    Global,
    Repo,
}

impl HookConfigScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Repo => "repo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryHookContext<'a> {
    pub name: &'a str,
    pub source_dir: &'a str,
    pub worktree_path: &'a str,
    pub target_branch: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookScope<'a> {
    Workspace,
    Repository(RepositoryHookContext<'a>),
}

pub struct HookInvocation<'a> {
    hook: &'a HookValue,
    stage: HookStage,
    operation: HookOperation,
    config_scope: HookConfigScope,
    workspace_status: WorkspaceStatus,
    workspace: &'a WorkspaceConfig,
    scope: HookScope<'a>,
}

impl<'a> HookInvocation<'a> {
    pub fn for_workspace(
        hook: Option<&'a HookValue>,
        stage: HookStage,
        operation: HookOperation,
        workspace_status: WorkspaceStatus,
        workspace: &'a WorkspaceConfig,
    ) -> Option<Self> {
        hook.map(|hook| Self {
            hook,
            stage,
            operation,
            config_scope: HookConfigScope::Global,
            workspace_status,
            workspace,
            scope: HookScope::Workspace,
        })
    }

    pub fn for_repository(
        repo_hook: Option<&'a HookValue>,
        global_hook: Option<&'a HookValue>,
        stage: HookStage,
        operation: HookOperation,
        workspace_status: WorkspaceStatus,
        workspace: &'a WorkspaceConfig,
        repository: RepositoryHookContext<'a>,
    ) -> Option<Self> {
        let (hook, config_scope) = match repo_hook {
            Some(hook) => (hook, HookConfigScope::Repo),
            None => (global_hook?, HookConfigScope::Global),
        };
        Some(Self {
            hook,
            stage,
            operation,
            config_scope,
            workspace_status,
            workspace,
            scope: HookScope::Repository(repository),
        })
    }

    fn cwd(&self) -> String {
        match self.scope {
            HookScope::Workspace => shellexpand::tilde(&self.workspace.workspace_dir).into_owned(),
            HookScope::Repository(repository) => {
                shellexpand::tilde(repository.worktree_path).into_owned()
            }
        }
    }

    fn env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::from([
            ("ZOOTREE_HOOK".into(), self.stage.as_str().into()),
            ("ZOOTREE_OPERATION".into(), self.operation.as_str().into()),
            (
                "ZOOTREE_HOOK_CONFIG_SCOPE".into(),
                self.config_scope.as_str().into(),
            ),
            ("ZOOTREE_WORKSPACE".into(), self.workspace.name.clone()),
            (
                "ZOOTREE_WORKSPACE_TITLE".into(),
                self.workspace.title.clone(),
            ),
            (
                "ZOOTREE_WORKSPACE_DESCRIPTION".into(),
                self.workspace.description.clone(),
            ),
            (
                "ZOOTREE_WORKSPACE_STATUS".into(),
                self.workspace_status.as_str().into(),
            ),
            (
                "ZOOTREE_WORKSPACE_DIR".into(),
                shellexpand::tilde(&self.workspace.workspace_dir).into_owned(),
            ),
            ("ZOOTREE_BRANCH".into(), self.workspace.branch.clone()),
            ("ZOOTREE_VERSION".into(), env!("CARGO_PKG_VERSION").into()),
        ]);

        match self.scope {
            HookScope::Workspace => {
                env.insert("ZOOTREE_HOOK_SCOPE".into(), "workspace".into());
            }
            HookScope::Repository(repository) => {
                env.insert("ZOOTREE_HOOK_SCOPE".into(), "repo".into());
                env.insert("ZOOTREE_REPO".into(), repository.name.into());
                env.insert(
                    "ZOOTREE_REPO_SOURCE_DIR".into(),
                    shellexpand::tilde(repository.source_dir).into_owned(),
                );
                env.insert(
                    "ZOOTREE_WORKTREE_PATH".into(),
                    shellexpand::tilde(repository.worktree_path).into_owned(),
                );
                if let Some(target_branch) = repository.target_branch {
                    env.insert("ZOOTREE_TARGET_BRANCH".into(), target_branch.into());
                }
            }
        }
        env
    }
}

pub struct HookEngine<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> HookEngine<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    pub fn execute(&self, invocation: &HookInvocation<'_>) -> Result<()> {
        let (program, args) = match invocation.hook {
            HookValue::Simple(cmd) => ("sh".to_string(), vec!["-c".to_string(), cmd.clone()]),
            HookValue::File { file } => (
                "sh".to_string(),
                vec![shellexpand::tilde(file).into_owned()],
            ),
            HookValue::Inline { inline } => {
                ("sh".to_string(), vec!["-c".to_string(), inline.clone()])
            }
        };

        let spec = CommandSpec {
            program,
            args,
            cwd: Some(invocation.cwd()),
            env: invocation.env_vars(),
            env_remove: OFFICIAL_HOOK_ENV_VARS
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        };

        let output = self.runner.run(&spec)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("hook failed: {}", stderr);
        }
        Ok(())
    }
}
