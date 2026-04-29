use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{Result, bail};
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
            args: full_args,
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git command failed: {}", stderr);
        }
        Ok(output)
    }

    pub fn worktree_add(&self, repo_path: &str, branch: &str, worktree_path: &str, base: &str) -> Result<()> {
        self.git(repo_path, vec!["worktree", "add", "-b", branch, worktree_path, base])?;
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

    pub fn merge(&self, repo_path: &str, branch: &str, target: &str, strategy: Option<&str>) -> Result<()> {
        self.git(repo_path, vec!["checkout", target])?;
        match strategy {
            Some("squash") => {
                self.git(repo_path, vec!["merge", "--squash", branch])?;
                self.git(repo_path, vec!["commit", "-m", &format!("squash merge {}", branch)])?;
            }
            Some("rebase") => {
                self.git(repo_path, vec!["rebase", branch])?;
            }
            _ => {
                self.git(repo_path, vec!["merge", branch])?;
            }
        }
        Ok(())
    }

    pub fn push(&self, repo_path: &str, branch: &str) -> Result<()> {
        self.git(repo_path, vec!["push", "origin", branch])?;
        Ok(())
    }

    pub fn delete_remote_branch(&self, repo_path: &str, branch: &str) -> Result<()> {
        self.git(repo_path, vec!["push", "origin", "--delete", branch])?;
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
            args: vec!["-C".into(), worktree_path.into(), "status".into(), "--porcelain".into()],
            cwd: None,
            env: HashMap::new(),
        };
        let output = self.runner.run(&spec)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }
}
