use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

pub(in crate::core) fn is_inside_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some() || std::env::var_os("ZELLIJ_SESSION_NAME").is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::core) enum SessionLookup {
    NotFound,
    Unique,
    Ambiguous,
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

pub(in crate::core) struct ZellijCommands<'a, R: CommandRunner> {
    runner: &'a R,
    in_zellij: bool,
}

impl<'a, R: CommandRunner> ZellijCommands<'a, R> {
    pub(in crate::core) fn new(runner: &'a R, in_zellij: bool) -> Self {
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

    pub(in crate::core) fn lookup_session(&self, session_name: &str) -> Result<SessionLookup> {
        let output = self.zellij(vec!["list-sessions".into()])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let err_output = if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            };
            bail!("zellij list-sessions failed: {}", err_output);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let matches = stdout
            .lines()
            .filter(|line| session_list_line_matches(line, session_name))
            .count();
        Ok(match matches {
            0 => SessionLookup::NotFound,
            1 => SessionLookup::Unique,
            _ => SessionLookup::Ambiguous,
        })
    }

    pub(in crate::core) fn activate_existing(
        &self,
        session_name: &str,
        workspace_name: &str,
    ) -> Result<()> {
        if self.in_zellij {
            println!("zellij session '{}' already exists.", session_name);
            println!(
                "Run `zootree open {}` (outside zellij) to attach.",
                workspace_name
            );
            Ok(())
        } else {
            self.attach_session(session_name)
        }
    }

    pub(in crate::core) fn create_session(
        &self,
        session_name: &str,
        workspace_name: &str,
        layout_file: &Path,
    ) -> Result<()> {
        if self.in_zellij {
            self.start_session_background(session_name, layout_file)?;
            println!(
                "zellij session '{}' is running in background.",
                session_name
            );
            println!(
                "Run `zootree open {}` (outside zellij) to attach.",
                workspace_name
            );
            Ok(())
        } else {
            self.start_session(session_name, layout_file)
        }
    }

    pub(in crate::core) fn delete_session_checked(&self, session_name: &str) -> Result<()> {
        info!("killing zellij session: {}", session_name);
        let output = self.zellij(vec![
            "delete-session".into(),
            "--force".into(),
            session_name.into(),
        ])?;
        if !output.status.success() {
            bail!(
                "zellij delete-session failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
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

    #[test]
    fn create_foreground_session_translates_to_interactive_command() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"", b""));

        ZellijCommands::new(&runner, false)
            .create_session("zootree-fair-fox", "fair-fox", Path::new("/tmp/layout.kdl"))
            .unwrap();

        let calls = runner.take_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "zellij");
        assert_eq!(
            calls[0].args,
            vec![
                "--new-session-with-layout",
                "/tmp/layout.kdl",
                "--session",
                "zootree-fair-fox"
            ]
        );
    }

    #[test]
    fn create_background_session_removes_parent_zellij_environment() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"", b""));

        ZellijCommands::new(&runner, true)
            .create_session("zootree-fair-fox", "fair-fox", Path::new("/tmp/layout.kdl"))
            .unwrap();

        let calls = runner.take_calls();
        assert_eq!(
            calls[0].args,
            vec![
                "-l",
                "/tmp/layout.kdl",
                "attach",
                "--create-background",
                "zootree-fair-fox"
            ]
        );
        assert_eq!(
            calls[0].env_remove,
            vec!["ZELLIJ", "ZELLIJ_SESSION_NAME", "ZELLIJ_PANE_ID"]
        );
    }

    #[test]
    fn activate_existing_outside_zellij_attaches_interactively() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"", b""));

        ZellijCommands::new(&runner, false)
            .activate_existing("zootree-fair-fox", "fair-fox")
            .unwrap();

        let calls = runner.take_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args, vec!["attach", "zootree-fair-fox"]);
    }

    #[test]
    fn lookup_session_requires_an_exact_unique_name() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            b"backup-zootree-fair-fox\nzootree-fair-fox [Created 1m ago]\n",
            b"",
        ));

        let lookup = ZellijCommands::new(&runner, false)
            .lookup_session("zootree-fair-fox")
            .unwrap();

        assert_eq!(lookup, SessionLookup::Unique);
        assert_eq!(runner.take_calls()[0].args, vec!["list-sessions"]);
    }

    #[test]
    fn lookup_session_preserves_ambiguous_result() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            b"zootree-fair-fox\nzootree-fair-fox [Created 1m ago]\n",
            b"",
        ));

        let lookup = ZellijCommands::new(&runner, false)
            .lookup_session("zootree-fair-fox")
            .unwrap();

        assert_eq!(lookup, SessionLookup::Ambiguous);
    }

    #[test]
    fn lookup_session_handles_ansi_decorated_names() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            b"\x1b[32mzootree-fair-fox\x1b[0m [Created 1m ago]\n",
            b"",
        ));

        let lookup = ZellijCommands::new(&runner, false)
            .lookup_session("zootree-fair-fox")
            .unwrap();

        assert_eq!(lookup, SessionLookup::Unique);
    }

    #[test]
    fn delete_session_translates_force_flag_and_propagates_failure() {
        let runner = MockRunner::new();
        runner.push_response(output(1, b"", b"permission denied"));

        let error = ZellijCommands::new(&runner, false)
            .delete_session_checked("zootree-fair-fox")
            .unwrap_err();

        assert!(error.to_string().contains("permission denied"));
        assert_eq!(
            runner.take_calls()[0].args,
            vec!["delete-session", "--force", "zootree-fair-fox"]
        );
    }
}
