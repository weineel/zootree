use crate::runner::{CommandRunner, CommandSpec, OutputLine};
use anyhow::{bail, Result};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktree {
    pub path: String,
    pub branch: Option<String>,
}

pub struct GitOps<'a, R: CommandRunner> {
    runner: &'a R,
    on_line: Option<&'a dyn Fn(&OutputLine)>,
}

impl<'a, R: CommandRunner> GitOps<'a, R> {
    pub fn new(runner: &'a R) -> Self {
        Self {
            runner,
            on_line: None,
        }
    }

    /// Forwards output lines of the git commands that change state (merge,
    /// push, worktree changes) to `on_line` while they run.
    pub fn with_output(runner: &'a R, on_line: &'a dyn Fn(&OutputLine)) -> Self {
        Self {
            runner,
            on_line: Some(on_line),
        }
    }

    fn git(&self, repo_path: &str, args: Vec<&str>) -> Result<std::process::Output> {
        let mut full_args = vec!["-C".to_string(), repo_path.to_string()];
        full_args.extend(args.into_iter().map(String::from));
        let spec = CommandSpec {
            program: "git".into(),
            args: full_args.clone(),
            cwd: None,
            env: HashMap::new(),
            env_remove: vec![],
        };
        let output = match self.on_line {
            Some(on_line) => self.runner.run_streaming(&spec, &mut |l| on_line(&l))?,
            None => self.runner.run(&spec)?,
        };
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
        // An abbreviated ref becomes "heads/main" when a tag is also named "main".
        let output = self.git(repo_path, vec!["rev-parse", "--symbolic-full-name", "HEAD"])?;
        let reference = String::from_utf8_lossy(&output.stdout);
        let reference = reference.trim();
        Ok(reference
            .strip_prefix("refs/heads/")
            .unwrap_or(reference)
            .to_owned())
    }

    pub fn short_revision(&self, repo_path: &str, revision: &str) -> Result<String> {
        let output = self.git(repo_path, vec!["rev-parse", "--short", revision])?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Full object name of `revision` (used to tie records to a branch HEAD).
    pub fn revision(&self, repo_path: &str, revision: &str) -> Result<String> {
        let output = self.git(repo_path, vec!["rev-parse", "--verify", revision])?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn repo_root(&self, repo_path: &str) -> Result<String> {
        let output = self.git(repo_path, vec!["rev-parse", "--show-toplevel"])?;
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(root)
    }

    pub fn branch_exists(&self, repo_path: &str, branch: &str) -> Result<bool> {
        let refname = format!("refs/heads/{}", branch);
        let output = self.git(
            repo_path,
            vec!["for-each-ref", "--format=%(refname)", &refname],
        )?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().any(|line| line.trim() == refname))
    }

    pub fn remote_branches(&self, repo_path: &str, branch: &str) -> Result<Vec<String>> {
        let output = self.git(
            repo_path,
            vec!["for-each-ref", "--format=%(refname)", "refs/remotes"],
        )?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().strip_prefix("refs/remotes/"))
            .filter(|name| name.split_once('/').is_some_and(|(_, rest)| rest == branch))
            .map(str::to_string)
            .collect())
    }

    pub fn branch_ref_exists(&self, repo_path: &str, branch: &str) -> Result<bool> {
        let local_ref = format!("refs/heads/{branch}");
        let remote_ref = format!("refs/remotes/{branch}");
        let output = self.git(
            repo_path,
            vec![
                "for-each-ref",
                "--format=%(refname)",
                &local_ref,
                &remote_ref,
            ],
        )?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| matches!(line.trim(), value if value == local_ref || value == remote_ref)))
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

    pub fn worktree_add_existing(
        &self,
        repo_path: &str,
        branch: &str,
        worktree_path: &str,
    ) -> Result<()> {
        self.git(repo_path, vec!["worktree", "add", worktree_path, branch])?;
        Ok(())
    }

    pub fn worktree_add_tracking(
        &self,
        repo_path: &str,
        branch: &str,
        worktree_path: &str,
        remote_branch: &str,
    ) -> Result<()> {
        self.git(
            repo_path,
            vec![
                "worktree",
                "add",
                "--track",
                "-b",
                branch,
                worktree_path,
                remote_branch,
            ],
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

    pub fn worktrees(&self, repo_path: &str) -> Result<Vec<GitWorktree>> {
        let output = self.git(repo_path, vec!["worktree", "list", "--porcelain", "-z"])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .split("\0\0")
            .filter_map(|record| {
                let mut path = None;
                let mut branch = None;
                for field in record.split('\0') {
                    if let Some(value) = field.strip_prefix("worktree ") {
                        path = Some(value.to_string());
                    } else if let Some(value) = field.strip_prefix("branch refs/heads/") {
                        branch = Some(value.to_string());
                    }
                }
                path.map(|path| GitWorktree { path, branch })
            })
            .collect())
    }

    pub fn worktree_registered_for_branch(
        &self,
        repo_path: &str,
        worktree_path: &str,
        branch: &str,
    ) -> Result<bool> {
        let output = self.git(repo_path, vec!["worktree", "list", "--porcelain", "-z"])?;
        let expected_branch = format!("refs/heads/{branch}");
        let mut matched_path = false;
        for field in output.stdout.split(|byte| *byte == b'\0') {
            if field.is_empty() {
                matched_path = false;
            } else if let Some(path) = field.strip_prefix(b"worktree ") {
                matched_path = path == worktree_path.as_bytes();
            } else if matched_path {
                if let Some(recorded_branch) = field.strip_prefix(b"branch ") {
                    if recorded_branch == expected_branch.as_bytes() {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
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

    /// Integrates `branch` into `target`. When a step fails after the
    /// checkout, the half-integrated state is rolled back on a best-effort
    /// basis so the Source checkout is not left mid-merge.
    pub fn merge_with_worktree(
        &self,
        repo_path: &str,
        branch_worktree_path: Option<&str>,
        branch: &str,
        target: &str,
        strategy: Option<&str>,
        message: &str,
    ) -> Result<()> {
        let started = std::cell::Cell::new(false);
        let result = self.merge_steps(
            repo_path,
            branch_worktree_path,
            branch,
            target,
            strategy,
            message,
            &started,
        );
        if result.is_err() && started.get() {
            self.recover_failed_merge(repo_path, branch_worktree_path, strategy);
        }
        result
    }

    fn recover_failed_merge(
        &self,
        repo_path: &str,
        branch_worktree_path: Option<&str>,
        strategy: Option<&str>,
    ) {
        // Errors are ignored on purpose: the original failure is what matters.
        let _ = match strategy {
            Some("rebase") => match branch_worktree_path {
                Some(worktree) => self.git(worktree, vec!["rebase", "--abort"]),
                None => return,
            },
            Some("merge") => self.git(repo_path, vec!["merge", "--abort"]),
            _ => self.git(repo_path, vec!["reset", "--merge"]),
        };
    }

    #[allow(clippy::too_many_arguments)]
    fn merge_steps(
        &self,
        repo_path: &str,
        branch_worktree_path: Option<&str>,
        branch: &str,
        target: &str,
        strategy: Option<&str>,
        message: &str,
        started: &std::cell::Cell<bool>,
    ) -> Result<()> {
        match strategy {
            Some("rebase") => {
                let branch_worktree_path = branch_worktree_path.ok_or_else(|| {
                    anyhow::anyhow!("rebase strategy requires branch worktree path")
                })?;
                started.set(true);
                self.git(branch_worktree_path, vec!["rebase", target])?;
                started.set(false);
                self.git(repo_path, vec!["checkout", target])?;
                started.set(true);
                self.git(repo_path, vec!["merge", "--ff-only", branch])?;
            }
            Some("merge") => {
                self.git(repo_path, vec!["checkout", target])?;
                started.set(true);
                self.git(repo_path, vec!["merge", branch])?;
            }
            _ => {
                self.git(repo_path, vec!["checkout", target])?;
                started.set(true);
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
                        env_remove: vec![],
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

    /// Whether tracked files differ from HEAD (untracked files ignored).
    /// Used to tell if a checkout of another branch would be refused.
    pub fn has_tracked_changes(&self, repo_path: &str) -> Result<bool> {
        let output = self.git(
            repo_path,
            vec!["status", "--porcelain", "--untracked-files=no"],
        )?;
        Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
    }

    pub fn has_uncommitted_changes(&self, worktree_path: &str) -> Result<bool> {
        let output = self.git(worktree_path, vec!["status", "--porcelain"])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }
}
