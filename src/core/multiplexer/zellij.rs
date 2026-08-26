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

    pub(in crate::core) fn tab_names(&self, session_name: &str) -> Result<Vec<String>> {
        let output = self.zellij(vec![
            "--session".into(),
            session_name.into(),
            "action".into(),
            "query-tab-names".into(),
        ])?;
        if !output.status.success() {
            bail!(
                "zellij query-tab-names failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect())
    }

    pub(in crate::core) fn create_tab(
        &self,
        session_name: &str,
        layout_path: &Path,
        tab_name: &str,
    ) -> Result<String> {
        let output = self.zellij(vec![
            "--session".into(),
            session_name.into(),
            "action".into(),
            "new-tab".into(),
            "--layout".into(),
            layout_path.to_string_lossy().into_owned(),
            "--name".into(),
            tab_name.into(),
        ])?;
        if !output.status.success() {
            bail!(
                "zellij new-tab failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let tab_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !tab_id.is_empty() && tab_id.chars().all(|character| character.is_ascii_digit()) {
            return Ok(tab_id);
        }

        let primary_error = anyhow::anyhow!(
            "zellij new-tab did not return a numeric tab ID: '{}'",
            tab_id
        );
        let matches = match self.tab_names(session_name) {
            Ok(names) => names.into_iter().filter(|name| name == tab_name).count(),
            Err(inspection_error) => {
                return Err(anyhow::anyhow!(
                    "{primary_error:#}; rollback residue: failed to inspect Zellij tabs after creation: {inspection_error:#}"
                ));
            }
        };
        match matches {
            0 => Err(anyhow::anyhow!(
                "{primary_error:#}; rollback residue: the created Zellij tab could not be identified"
            )),
            1 => Err(anyhow::anyhow!(
                "{primary_error:#}; rollback residue: a tab named '{tab_name}' now exists in session '{session_name}', but it could not be closed safely without a stable tab ID"
            )),
            _ => Err(anyhow::anyhow!(
                "{primary_error:#}; rollback residue: tab name '{tab_name}' is ambiguous in session '{session_name}'"
            )),
        }
    }

    pub(in crate::core) fn close_tab(&self, session_name: &str, tab_id: &str) -> Result<()> {
        let output = self.zellij(vec![
            "--session".into(),
            session_name.into(),
            "action".into(),
            "close-tab".into(),
            "--tab-id".into(),
            tab_id.into(),
        ])?;
        if !output.status.success() {
            bail!(
                "zellij close-tab failed: {}",
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

    #[test]
    fn create_tab_passes_the_name_and_returns_the_numeric_id() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"7\n", b""));

        let tab_id = ZellijCommands::new(&runner, false)
            .create_tab("zootree-fair-fox", Path::new("/tmp/backend.kdl"), "backend")
            .unwrap();

        assert_eq!(tab_id, "7");
        assert_eq!(
            runner.take_calls()[0].args,
            vec![
                "--session",
                "zootree-fair-fox",
                "action",
                "new-tab",
                "--layout",
                "/tmp/backend.kdl",
                "--name",
                "backend"
            ]
        );
    }

    #[test]
    fn malformed_success_response_reports_residue_without_closing_by_focus() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"created\n", b""));
        runner.push_response(output(0, b"overview\nbackend\n", b""));

        let error = ZellijCommands::new(&runner, false)
            .create_tab("zootree-fair-fox", Path::new("/tmp/backend.kdl"), "backend")
            .unwrap_err();

        assert!(error.to_string().contains("numeric tab ID"));
        assert!(error.to_string().contains("rollback residue"));
        assert!(error.to_string().contains("stable tab ID"));
        let calls = runner.take_calls();
        assert_eq!(
            calls[1].args,
            vec!["--session", "zootree-fair-fox", "action", "query-tab-names"]
        );
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn failed_create_does_not_inspect_or_close_tabs() {
        let runner = MockRunner::new();
        runner.push_response(output(1, b"", b"layout rejected"));

        let error = ZellijCommands::new(&runner, false)
            .create_tab("zootree-fair-fox", Path::new("/tmp/backend.kdl"), "backend")
            .unwrap_err();

        assert!(error.to_string().contains("layout rejected"));
        assert_eq!(runner.take_calls().len(), 1);
    }
}
