use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

pub struct GitOps<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> GitOps<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    fn git(&self, repo_path: &str, args: Vec<&str>) -> Result<std::process::Output> {
        let mut full_args = vec!["-C".to_string(), repo_path.to_string()];
        full_args.extend(args.into_iter().map(String::from));
        let spec = CommandSpec {
            program: "git".into(),
            args: full_args.clone(),
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let err_output = if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            };
            let cmd = format!("git {}", full_args.join(" "));
            bail!(
                "git command failed:\n  command: {}\n  error: {}",
                cmd,
                err_output
            );
        }
        Ok(output)
    }

    pub fn current_branch(&self, repo_path: &str) -> Result<String> {
        let output = self.git(repo_path, vec!["rev-parse", "--abbrev-ref", "HEAD"])?;
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(branch)
    }

    pub fn repo_root(&self, repo_path: &str) -> Result<String> {
        let output = self.git(repo_path, vec!["rev-parse", "--show-toplevel"])?;
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(root)
    }

    pub fn branch_exists(&self, repo_path: &str, branch: &str) -> Result<bool> {
        let spec = CommandSpec {
            program: "git".into(),
            args: vec![
                "-C".into(),
                repo_path.into(),
                "rev-parse".into(),
                "--verify".into(),
                format!("refs/heads/{}", branch),
            ],
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        Ok(output.status.success())
    }

    pub fn worktree_add(
        &self,
        repo_path: &str,
        branch: &str,
        worktree_path: &str,
        base: &str,
    ) -> Result<()> {
        self.git(
            repo_path,
            vec!["worktree", "add", "-b", branch, worktree_path, base],
        )?;
        Ok(())
    }

    pub fn worktree_remove(&self, repo_path: &str, worktree_path: &str, force: bool) -> Result<()> {
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(worktree_path);
        self.git(repo_path, args)?;
        Ok(())
    }

    pub fn merge(
        &self,
        repo_path: &str,
        branch: &str,
        target: &str,
        strategy: Option<&str>,
        message: &str,
    ) -> Result<()> {
        self.merge_with_worktree(repo_path, None, branch, target, strategy, message)
    }

    pub fn merge_with_worktree(
        &self,
        repo_path: &str,
        branch_worktree_path: Option<&str>,
        branch: &str,
        target: &str,
        strategy: Option<&str>,
        message: &str,
    ) -> Result<()> {
        match strategy {
            Some("rebase") => {
                let branch_worktree_path = branch_worktree_path.ok_or_else(|| {
                    anyhow::anyhow!("rebase strategy requires branch worktree path")
                })?;
                self.git(branch_worktree_path, vec!["rebase", target])?;
                self.git(repo_path, vec!["checkout", target])?;
                self.git(repo_path, vec!["merge", "--ff-only", branch])?;
            }
            Some("merge") => {
                self.git(repo_path, vec!["checkout", target])?;
                self.git(repo_path, vec!["merge", branch])?;
            }
            _ => {
                self.git(repo_path, vec!["checkout", target])?;
                // 默认使用 squash 方式
                self.git(repo_path, vec!["merge", "--squash", branch])?;
                // exit 1 表示有 staged 变更，exit 0 表示无变更（已是最新）
                let has_staged = {
                    let mut args = vec!["-C".to_string(), repo_path.to_string()];
                    args.extend(
                        ["diff", "--staged", "--quiet"]
                            .iter()
                            .map(|s| s.to_string()),
                    );
                    let spec = CommandSpec {
                        program: "git".into(),
                        args,
                        cwd: None,
                        env: HashMap::new(),
                    };
                    let output = self.runner.run(&spec)?;
                    !output.status.success()
                };
                if has_staged {
                    self.git(repo_path, vec!["commit", "-m", message])?;
                } else {
                    tracing::warn!("nothing to merge from '{}' into '{}'", branch, target);
                }
            }
        }
        Ok(())
    }

    pub fn push(&self, repo_path: &str, branch: &str) -> Result<()> {
        self.git(repo_path, vec!["push", "origin", branch])?;
        Ok(())
    }

    pub fn delete_local_branch(&self, repo_path: &str, branch: &str, force: bool) -> Result<()> {
        let flag = if force { "-D" } else { "-d" };
        self.git(repo_path, vec!["branch", flag, branch])?;
        Ok(())
    }

    pub fn has_uncommitted_changes(&self, worktree_path: &str) -> Result<bool> {
        let spec = CommandSpec {
            program: "git".into(),
            args: vec![
                "-C".into(),
                worktree_path.into(),
                "status".into(),
                "--porcelain".into(),
            ],
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }
}
