use crate::config::global::HookValue;
use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

pub struct HookContext {
    pub workspace: String,
    pub repo: Option<String>,
    pub branch: String,
    pub target_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub workspace_dir: String,
}

impl HookContext {
    fn env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("ZOOTREE_WORKSPACE".into(), self.workspace.clone());
        if let Some(repo) = &self.repo {
            env.insert("ZOOTREE_REPO".into(), repo.clone());
        }
        env.insert("ZOOTREE_BRANCH".into(), self.branch.clone());
        if let Some(tb) = &self.target_branch {
            env.insert("ZOOTREE_TARGET_BRANCH".into(), tb.clone());
        }
        if let Some(wp) = &self.worktree_path {
            env.insert("ZOOTREE_WORKTREE_PATH".into(), wp.clone());
        }
        env.insert("ZOOTREE_WORKSPACE_DIR".into(), self.workspace_dir.clone());
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

    pub fn execute(&self, hook: &HookValue, ctx: &HookContext) -> Result<()> {
        let (program, args) = match hook {
            HookValue::Simple(cmd) => ("sh".to_string(), vec!["-c".to_string(), cmd.clone()]),
            HookValue::File { file } => ("sh".to_string(), vec![file.clone()]),
            HookValue::Inline { inline } => {
                ("sh".to_string(), vec!["-c".to_string(), inline.clone()])
            }
        };

        let cwd = ctx
            .worktree_path
            .clone()
            .or_else(|| Some(ctx.workspace_dir.clone()));

        let spec = CommandSpec {
            program,
            args,
            cwd,
            env: ctx.env_vars(),
        };

        let output = self.runner.run(&spec)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("hook failed: {}", stderr);
        }
        Ok(())
    }

    pub fn execute_if_set(&self, hook: &Option<HookValue>, ctx: &HookContext) -> Result<()> {
        if let Some(h) = hook {
            self.execute(h, ctx)?;
        }
        Ok(())
    }
}
