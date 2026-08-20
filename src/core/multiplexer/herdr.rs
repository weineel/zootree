use crate::runner::{CommandRunner, CommandSpec};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

const MINIMUM_VERSION: (u64, u64, u64) = (0, 8, 0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct HerdrWorkspace {
    pub(in crate::core) id: String,
    pub(in crate::core) label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct CreatedWorkspace {
    pub(in crate::core) workspace: HerdrWorkspace,
    pub(in crate::core) tab_id: String,
    pub(in crate::core) root_pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct RepoSpec {
    pub(in crate::core) name: String,
    pub(in crate::core) cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct EnvironmentSpec {
    pub(in crate::core) session: String,
    pub(in crate::core) label: String,
    pub(in crate::core) workspace_cwd: String,
    pub(in crate::core) info_command: String,
    pub(in crate::core) repos: Vec<RepoSpec>,
    pub(in crate::core) agent_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct CreatedRepoTab {
    pub(in crate::core) tab_id: String,
    pub(in crate::core) primary_pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::core) struct CreatedEnvironment {
    pub(in crate::core) workspace: HerdrWorkspace,
    pub(in crate::core) overview_tab_id: String,
    pub(in crate::core) overview_info_pane_id: String,
    pub(in crate::core) overview_primary_pane_id: String,
    pub(in crate::core) repo_tabs: Vec<CreatedRepoTab>,
    pub(in crate::core) agent_pane_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SuccessEnvelope<T> {
    result: T,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceInfoResult {
    #[serde(rename = "type")]
    kind: String,
    workspace: WorkspaceRecord,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResult {
    #[serde(rename = "type")]
    kind: String,
    workspaces: Vec<WorkspaceRecord>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceCreatedResult {
    #[serde(rename = "type")]
    kind: String,
    workspace: WorkspaceRecord,
    tab: TabRecord,
    root_pane: PaneRecord,
}

#[derive(Debug, Deserialize)]
struct TabCreatedResult {
    #[serde(rename = "type")]
    kind: String,
    tab: TabRecord,
    root_pane: PaneRecord,
}

#[derive(Debug, Deserialize)]
struct TabInfoResult {
    #[serde(rename = "type")]
    kind: String,
    tab: TabRecord,
}

#[derive(Debug, Deserialize)]
struct PaneInfoResult {
    #[serde(rename = "type")]
    kind: String,
    pane: PaneRecord,
}

#[derive(Debug, Deserialize)]
struct PaneFocusDirectionResult {
    #[serde(rename = "type")]
    kind: String,
    focus: PaneFocusDirection,
}

#[derive(Debug, Deserialize)]
struct PaneFocusDirection {
    changed: bool,
    source_pane_id: String,
    focused_pane_id: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentInfoResult {
    #[serde(rename = "type")]
    kind: String,
    agent: AgentRecord,
}

#[derive(Debug, Deserialize)]
struct AgentRecord {
    pane_id: String,
}

#[derive(Debug, Deserialize)]
struct OkResult {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceCreateRecoveryEnvelope {
    result: WorkspaceCreateRecoveryResult,
}

#[derive(Debug, Deserialize)]
struct WorkspaceCreateRecoveryResult {
    workspace: WorkspaceIdRecord,
}

#[derive(Debug, Deserialize)]
struct SessionList {
    sessions: Vec<SessionRecord>,
}

#[derive(Debug, Deserialize)]
struct SessionRecord {
    name: String,
    running: bool,
    socket_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceRecord {
    workspace_id: String,
    label: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceIdRecord {
    workspace_id: String,
}

#[derive(Debug, Deserialize)]
struct TabRecord {
    tab_id: String,
}

#[derive(Debug, Deserialize)]
struct PaneRecord {
    pane_id: String,
}

pub(in crate::core) struct HerdrCommands<'a, R: CommandRunner> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> HerdrCommands<'a, R> {
    pub(in crate::core) fn new(runner: &'a R) -> Self {
        Self { runner }
    }

    fn run(&self, args: Vec<String>) -> Result<std::process::Output> {
        self.runner
            .run(&CommandSpec {
                program: "herdr".into(),
                args,
                cwd: None,
                env: HashMap::new(),
                env_remove: Vec::new(),
            })
            .context("failed to execute Herdr; install Herdr 0.8.0+ or select another multiplexer")
    }

    fn run_session(&self, session: &str, args: Vec<String>) -> Result<std::process::Output> {
        let mut prefixed = vec!["--session".into(), session.into()];
        prefixed.extend(args);
        let output = self.run(prefixed)?;
        if !output.status.success()
            && matches!(
                error_body(&output)
                    .as_ref()
                    .map(|error| error.code.as_str()),
                Some("server_not_running" | "session_not_running")
            )
        {
            bail!(
                "Herdr named session '{session}' is unavailable: {}; run `herdr session attach {session}`",
                command_error_text(&output)
            );
        }
        Ok(output)
    }

    pub(in crate::core) fn ensure_supported_version(&self) -> Result<()> {
        let output = self.run(vec!["--version".into()])?;
        if !output.status.success() {
            bail!(
                "Herdr version check failed: {}",
                command_error_text(&output)
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout
            .split_whitespace()
            .find_map(parse_version)
            .ok_or_else(|| {
                anyhow::anyhow!("could not parse Herdr version from '{}'", stdout.trim())
            })?;
        if version < MINIMUM_VERSION {
            bail!(
                "Herdr {}.{}.{} is unsupported; zootree requires Herdr 0.8.0+",
                version.0,
                version.1,
                version.2
            );
        }
        Ok(())
    }

    fn request_workspace_create(
        &self,
        session: &str,
        cwd: &str,
        label: &str,
    ) -> Result<std::process::Output> {
        self.run_session(
            session,
            vec![
                "workspace".into(),
                "create".into(),
                "--cwd".into(),
                cwd.into(),
                "--label".into(),
                label.into(),
                "--no-focus".into(),
            ],
        )
    }

    pub(in crate::core) fn get_workspace(
        &self,
        session: &str,
        workspace_id: &str,
    ) -> Result<Option<HerdrWorkspace>> {
        let output = self.run_session(
            session,
            vec!["workspace".into(), "get".into(), workspace_id.into()],
        )?;
        if !output.status.success() {
            if error_body(&output)
                .as_ref()
                .map(|error| error.code.as_str())
                == Some("workspace_not_found")
            {
                return Ok(None);
            }
            bail!(
                "Herdr workspace get failed: {}",
                command_error_text(&output)
            );
        }
        let result: WorkspaceInfoResult = decode_success(&output, "Herdr workspace get")?;
        if result.kind != "workspace_info" {
            bail!(
                "Herdr workspace get returned unexpected result type '{}'",
                result.kind
            );
        }
        Ok(Some(HerdrWorkspace {
            id: result.workspace.workspace_id,
            label: result.workspace.label,
        }))
    }

    pub(in crate::core) fn list_workspaces(&self, session: &str) -> Result<Vec<HerdrWorkspace>> {
        let output = self.run_session(session, vec!["workspace".into(), "list".into()])?;
        let result: WorkspaceListResult = decode_success(&output, "Herdr workspace list")?;
        if result.kind != "workspace_list" {
            bail!(
                "Herdr workspace list returned unexpected result type '{}'",
                result.kind
            );
        }
        Ok(result
            .workspaces
            .into_iter()
            .map(|workspace| HerdrWorkspace {
                id: workspace.workspace_id,
                label: workspace.label,
            })
            .collect())
    }

    fn ensure_ok(&self, output: std::process::Output, context: &str) -> Result<()> {
        let result: OkResult = decode_success(&output, context)?;
        if result.kind != "ok" {
            bail!(
                "{context} returned unexpected result type '{}'",
                result.kind
            );
        }
        Ok(())
    }

    fn ensure_exit_success(&self, output: std::process::Output, context: &str) -> Result<()> {
        if !output.status.success() {
            bail!("{context} failed: {}", command_error_text(&output));
        }
        Ok(())
    }

    fn ensure_tab_info(
        &self,
        output: std::process::Output,
        context: &str,
        expected_tab_id: &str,
    ) -> Result<()> {
        let result: TabInfoResult = decode_success(&output, context)?;
        if result.kind != "tab_info" {
            bail!(
                "{context} returned unexpected result type '{}'",
                result.kind
            );
        }
        if result.tab.tab_id != expected_tab_id {
            bail!(
                "{context} returned tab '{}' instead of '{expected_tab_id}'",
                result.tab.tab_id
            );
        }
        Ok(())
    }

    fn ensure_agent_info(
        &self,
        output: std::process::Output,
        context: &str,
        expected_pane_id: &str,
    ) -> Result<()> {
        let result: AgentInfoResult = decode_success(&output, context)?;
        if result.kind != "agent_info" {
            bail!(
                "{context} returned unexpected result type '{}'",
                result.kind
            );
        }
        if result.agent.pane_id != expected_pane_id {
            bail!(
                "{context} returned pane '{}' instead of '{expected_pane_id}'",
                result.agent.pane_id
            );
        }
        Ok(())
    }

    fn rename_tab(&self, session: &str, tab_id: &str, label: &str) -> Result<()> {
        let output = self.run_session(
            session,
            vec!["tab".into(), "rename".into(), tab_id.into(), label.into()],
        )?;
        self.ensure_tab_info(output, "Herdr tab rename", tab_id)
    }

    fn create_tab(
        &self,
        session: &str,
        workspace_id: &str,
        cwd: &str,
        label: &str,
    ) -> Result<(String, String)> {
        let output = self.run_session(
            session,
            vec![
                "tab".into(),
                "create".into(),
                "--workspace".into(),
                workspace_id.into(),
                "--cwd".into(),
                cwd.into(),
                "--label".into(),
                label.into(),
                "--no-focus".into(),
            ],
        )?;
        let result: TabCreatedResult = decode_success(&output, "Herdr tab create")?;
        if result.kind != "tab_created" {
            bail!(
                "Herdr tab create returned unexpected result type '{}'",
                result.kind
            );
        }
        Ok((result.tab.tab_id, result.root_pane.pane_id))
    }

    fn split_pane(
        &self,
        session: &str,
        pane_id: &str,
        direction: &str,
        cwd: &str,
    ) -> Result<String> {
        let output = self.run_session(
            session,
            vec![
                "pane".into(),
                "split".into(),
                pane_id.into(),
                "--direction".into(),
                direction.into(),
                "--ratio".into(),
                "0.5".into(),
                "--cwd".into(),
                cwd.into(),
                "--no-focus".into(),
            ],
        )?;
        let result: PaneInfoResult = decode_success(&output, "Herdr pane split")?;
        if result.kind != "pane_info" {
            bail!(
                "Herdr pane split returned unexpected result type '{}'",
                result.kind
            );
        }
        Ok(result.pane.pane_id)
    }

    fn run_pane(&self, session: &str, pane_id: &str, command: &str) -> Result<()> {
        let output = self.run_session(
            session,
            vec!["pane".into(), "run".into(), pane_id.into(), command.into()],
        )?;
        self.ensure_exit_success(output, "Herdr pane run")
    }

    pub(in crate::core) fn close_workspace(&self, session: &str, workspace_id: &str) -> Result<()> {
        let output = self.run_session(
            session,
            vec!["workspace".into(), "close".into(), workspace_id.into()],
        )?;
        if !output.status.success()
            && error_body(&output)
                .as_ref()
                .map(|error| error.code.as_str())
                != Some("workspace_not_found")
        {
            bail!(
                "Herdr workspace close failed: {}",
                command_error_text(&output)
            );
        }
        if output.status.success() {
            self.ensure_ok(output, "Herdr workspace close")
        } else {
            Ok(())
        }
    }

    pub(in crate::core) fn create_environment(
        &self,
        spec: &EnvironmentSpec,
    ) -> Result<CreatedEnvironment> {
        if spec.repos.is_empty() {
            bail!("Herdr terminal environment requires at least one repository");
        }
        let create_output =
            self.request_workspace_create(&spec.session, &spec.workspace_cwd, &spec.label)?;
        let created = match decode_workspace_created(&create_output) {
            Ok(created) => created,
            Err(error) if !create_output.status.success() => return Err(error),
            Err(error) => {
                let recovery_id = workspace_id_from_create_output(&create_output)
                    .map(Some)
                    .map(Ok)
                    .unwrap_or_else(|| {
                        self.unique_workspace_id_by_label(&spec.session, &spec.label)
                    });
                return match recovery_id {
                    Ok(Some(workspace_id)) => {
                        self.rollback_error(&spec.session, &workspace_id, error)
                    }
                    Ok(None) => Err(anyhow::anyhow!(
                        "{error:#}; additionally failed to roll back the Herdr workspace because its create response had no workspace ID and no exact label match was found"
                    )),
                    Err(rollback_error) => Err(anyhow::anyhow!(
                        "{error:#}; additionally failed to identify the Herdr workspace for rollback: {rollback_error:#}"
                    )),
                };
            }
        };
        let build = (|| {
            self.rename_tab(&spec.session, &created.tab_id, "overview")?;
            let overview_primary_pane_id = self.split_pane(
                &spec.session,
                &created.root_pane_id,
                "right",
                &spec.workspace_cwd,
            )?;
            self.run_pane(&spec.session, &created.root_pane_id, &spec.info_command)?;

            let mut repo_tabs = Vec::with_capacity(spec.repos.len());
            for repo in &spec.repos {
                let (tab_id, primary_pane_id) =
                    self.create_tab(&spec.session, &created.workspace.id, &repo.cwd, &repo.name)?;
                let right_pane_id =
                    self.split_pane(&spec.session, &primary_pane_id, "right", &repo.cwd)?;
                self.split_pane(&spec.session, &right_pane_id, "down", &repo.cwd)?;
                repo_tabs.push(CreatedRepoTab {
                    tab_id,
                    primary_pane_id,
                });
            }

            let agent_pane_id = spec.agent_command.as_ref().map(|_| {
                if repo_tabs.len() == 1 {
                    repo_tabs[0].primary_pane_id.clone()
                } else {
                    overview_primary_pane_id.clone()
                }
            });
            if let (Some(command), Some(pane_id)) = (&spec.agent_command, &agent_pane_id) {
                self.run_pane(&spec.session, pane_id, command)?;
            }

            Ok(CreatedEnvironment {
                workspace: created.workspace.clone(),
                overview_tab_id: created.tab_id.clone(),
                overview_info_pane_id: created.root_pane_id.clone(),
                overview_primary_pane_id,
                repo_tabs,
                agent_pane_id,
            })
        })();

        match build {
            Ok(environment) => Ok(environment),
            Err(error) => self.rollback_error(&spec.session, &created.workspace.id, error),
        }
    }

    fn rollback_error(
        &self,
        session: &str,
        workspace_id: &str,
        error: anyhow::Error,
    ) -> Result<CreatedEnvironment> {
        match self.close_workspace(session, workspace_id) {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(anyhow::anyhow!(
                "{error:#}; additionally failed to roll back Herdr workspace '{workspace_id}': {rollback_error:#}"
            )),
        }
    }

    fn unique_workspace_id_by_label(&self, session: &str, label: &str) -> Result<Option<String>> {
        let mut matches = self
            .list_workspaces(session)?
            .into_iter()
            .filter(|workspace| workspace.label == label);
        let first = matches.next();
        if matches.next().is_some() {
            bail!(
                "Herdr workspace label '{label}' is ambiguous in named session '{session}'; refusing to guess during rollback"
            );
        }
        Ok(first.map(|workspace| workspace.id))
    }

    pub(in crate::core) fn focus_workspace(&self, session: &str, workspace_id: &str) -> Result<()> {
        let output = self.run_session(
            session,
            vec!["workspace".into(), "focus".into(), workspace_id.into()],
        )?;
        let result: WorkspaceInfoResult = decode_success(&output, "Herdr workspace focus")?;
        if result.kind != "workspace_info" {
            bail!(
                "Herdr workspace focus returned unexpected result type '{}'",
                result.kind
            );
        }
        if result.workspace.workspace_id != workspace_id {
            bail!(
                "Herdr workspace focus returned workspace '{}' instead of '{workspace_id}'",
                result.workspace.workspace_id
            );
        }
        Ok(())
    }

    pub(in crate::core) fn focus_tab(&self, session: &str, tab_id: &str) -> Result<()> {
        let output =
            self.run_session(session, vec!["tab".into(), "focus".into(), tab_id.into()])?;
        self.ensure_tab_info(output, "Herdr tab focus", tab_id)
    }

    pub(in crate::core) fn focus_right_from(
        &self,
        session: &str,
        source_pane_id: &str,
        expected_pane_id: &str,
    ) -> Result<()> {
        let output = self.run_session(
            session,
            vec![
                "pane".into(),
                "focus".into(),
                "--pane".into(),
                source_pane_id.into(),
                "--direction".into(),
                "right".into(),
            ],
        )?;
        let result: PaneFocusDirectionResult = decode_success(&output, "Herdr pane focus")?;
        if result.kind != "pane_focus_direction" {
            bail!(
                "Herdr pane focus returned unexpected result type '{}'",
                result.kind
            );
        }
        if result.focus.source_pane_id != source_pane_id {
            bail!(
                "Herdr pane focus returned source pane '{}' instead of '{source_pane_id}'",
                result.focus.source_pane_id
            );
        }
        if !result.focus.changed {
            let reason = result.focus.reason.as_deref().unwrap_or("unknown reason");
            bail!("Herdr pane focus did not move focus from pane '{source_pane_id}': {reason}");
        }
        let focused_pane_id = result
            .focus
            .focused_pane_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Herdr pane focus returned no focused pane ID"))?;
        if focused_pane_id != expected_pane_id {
            bail!(
                "Herdr pane focus returned focused pane '{focused_pane_id}' instead of '{expected_pane_id}'"
            );
        }
        Ok(())
    }

    pub(in crate::core) fn get_agent(&self, session: &str, pane_id: &str) -> Result<bool> {
        let output =
            self.run_session(session, vec!["agent".into(), "get".into(), pane_id.into()])?;
        if !output.status.success() {
            if error_body(&output)
                .as_ref()
                .map(|error| error.code.as_str())
                == Some("agent_not_found")
            {
                return Ok(false);
            }
            bail!("Herdr agent get failed: {}", command_error_text(&output));
        }
        self.ensure_agent_info(output, "Herdr agent get", pane_id)?;
        Ok(true)
    }

    pub(in crate::core) fn rename_agent(
        &self,
        session: &str,
        pane_id: &str,
        name: &str,
    ) -> Result<()> {
        let output = self.run_session(
            session,
            vec!["agent".into(), "rename".into(), pane_id.into(), name.into()],
        )?;
        self.ensure_agent_info(output, "Herdr agent rename", pane_id)
    }

    pub(in crate::core) fn attach_session(&self, session: &str) -> Result<()> {
        let status = self.runner.run_interactive(&CommandSpec {
            program: "herdr".into(),
            args: vec!["session".into(), "attach".into(), session.into()],
            cwd: None,
            env: HashMap::new(),
            env_remove: Vec::new(),
        })?;
        if !status.success() {
            let reason = status
                .code()
                .map(|code| format!("exit code {code}"))
                .unwrap_or_else(|| "terminated by signal".into());
            bail!("Herdr session attach failed with {reason}");
        }
        Ok(())
    }

    pub(in crate::core) fn session_socket(&self, session: &str) -> Result<Option<String>> {
        let output = self.run(vec!["session".into(), "list".into(), "--json".into()])?;
        if !output.status.success() {
            bail!("Herdr session list failed: {}", command_error_text(&output));
        }
        let list: SessionList = serde_json::from_slice(&output.stdout)
            .context("Herdr session list returned malformed JSON")?;
        Ok(list
            .sessions
            .into_iter()
            .find(|record| record.name == session && record.running)
            .and_then(|record| record.socket_path))
    }
}

fn parse_version(token: &str) -> Option<(u64, u64, u64)> {
    let core = token.trim_start_matches('v').split('-').next()?;
    let mut parts = core.split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

fn decode_workspace_created(output: &std::process::Output) -> Result<CreatedWorkspace> {
    let result: WorkspaceCreatedResult = decode_success(output, "Herdr workspace create")?;
    if result.kind != "workspace_created" {
        bail!(
            "Herdr workspace create returned unexpected result type '{}'",
            result.kind
        );
    }
    Ok(CreatedWorkspace {
        workspace: HerdrWorkspace {
            id: result.workspace.workspace_id,
            label: result.workspace.label,
        },
        tab_id: result.tab.tab_id,
        root_pane_id: result.root_pane.pane_id,
    })
}

fn workspace_id_from_create_output(output: &std::process::Output) -> Option<String> {
    serde_json::from_slice::<WorkspaceCreateRecoveryEnvelope>(&output.stdout)
        .ok()
        .map(|envelope| envelope.result.workspace.workspace_id)
        .filter(|workspace_id| !workspace_id.is_empty())
}

fn decode_success<T: for<'de> Deserialize<'de>>(
    output: &std::process::Output,
    context: &str,
) -> Result<T> {
    if !output.status.success() {
        bail!("{context} failed: {}", command_error_text(output));
    }
    let envelope: SuccessEnvelope<T> = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("{context} returned malformed JSON"))?;
    Ok(envelope.result)
}

fn command_error_text(output: &std::process::Output) -> String {
    if let Some(error) = error_body(output) {
        return format!("{}: {}", error.code, error.message);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr.trim().to_string()
    }
}

fn error_body(output: &std::process::Output) -> Option<ErrorBody> {
    serde_json::from_slice::<ErrorEnvelope>(&output.stderr)
        .or_else(|_| serde_json::from_slice::<ErrorEnvelope>(&output.stdout))
        .ok()
        .map(|envelope| envelope.error)
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
    fn supported_version_is_checked_before_session_commands() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"herdr 0.8.0\n", b""));

        HerdrCommands::new(&runner)
            .ensure_supported_version()
            .unwrap();

        let calls = runner.take_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "herdr");
        assert_eq!(calls[0].args, vec!["--version"]);
    }

    #[test]
    fn create_workspace_uses_named_session_and_decodes_all_creation_ids() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"id":"cli:workspace:create","result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#,
            b"",
        ));

        let commands = HerdrCommands::new(&runner);
        let output = commands
            .request_workspace_create("agents", "/tmp/rare-moon", "Support Herdr")
            .unwrap();
        let created = decode_workspace_created(&output).unwrap();

        assert_eq!(created.workspace.id, "w7");
        assert_eq!(created.workspace.label, "Support Herdr");
        assert_eq!(created.tab_id, "w7:t1");
        assert_eq!(created.root_pane_id, "w7:p1");
        let calls = runner.take_calls();
        assert_eq!(calls[0].program, "herdr");
        assert_eq!(
            calls[0].args,
            vec![
                "--session",
                "agents",
                "workspace",
                "create",
                "--cwd",
                "/tmp/rare-moon",
                "--label",
                "Support Herdr",
                "--no-focus",
            ]
        );
    }

    #[test]
    fn get_workspace_treats_the_documented_not_found_error_as_absent() {
        let runner = MockRunner::new();
        runner.push_response(output(
            1,
            b"",
            br#"{"id":"cli:workspace:get","error":{"code":"workspace_not_found","message":"workspace w7 not found"}}"#,
        ));

        let workspace = HerdrCommands::new(&runner)
            .get_workspace("agents", "w7")
            .unwrap();

        assert_eq!(workspace, None);
        assert_eq!(
            runner.take_calls()[0].args,
            vec!["--session", "agents", "workspace", "get", "w7"]
        );
    }

    #[test]
    fn list_workspaces_decodes_stable_ids_and_labels() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"workspace_id":"w2","label":"Other"},{"workspace_id":"w7","label":"Support Herdr"}]}}"#,
            b"",
        ));

        let workspaces = HerdrCommands::new(&runner)
            .list_workspaces("agents")
            .unwrap();

        assert_eq!(
            workspaces,
            vec![
                HerdrWorkspace {
                    id: "w2".into(),
                    label: "Other".into(),
                },
                HerdrWorkspace {
                    id: "w7".into(),
                    label: "Support Herdr".into(),
                },
            ]
        );
        assert_eq!(
            runner.take_calls()[0].args,
            vec!["--session", "agents", "workspace", "list"]
        );
    }

    #[test]
    fn single_repo_environment_builds_default_topology_and_routes_agent_to_repo_primary() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#,
            b"",
        ));
        runner.push_response(output(
            0,
            br#"{"result":{"type":"tab_info","tab":{"tab_id":"w7:t1"}}}"#,
            b"",
        ));
        runner.push_response(output(
            0,
            br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p2"}}}"#,
            b"",
        ));
        runner.push_response(output(0, b"", b""));
        runner.push_response(output(
            0,
            br#"{"result":{"type":"tab_created","tab":{"tab_id":"w7:t2"},"root_pane":{"pane_id":"w7:p3"}}}"#,
            b"",
        ));
        runner.push_response(output(
            0,
            br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p4"}}}"#,
            b"",
        ));
        runner.push_response(output(
            0,
            br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p5"}}}"#,
            b"",
        ));
        runner.push_response(output(0, b"", b""));

        let created = HerdrCommands::new(&runner)
            .create_environment(&EnvironmentSpec {
                session: "agents".into(),
                label: "Support Herdr".into(),
                workspace_cwd: "/tmp/rare-moon".into(),
                info_command: "zootree info rare-moon --watch".into(),
                repos: vec![RepoSpec {
                    name: "zootree".into(),
                    cwd: "/tmp/rare-moon/zootree".into(),
                }],
                agent_command: Some("codex -- 'Support Herdr'".into()),
            })
            .unwrap();

        assert_eq!(created.workspace.id, "w7");
        assert_eq!(created.overview_tab_id, "w7:t1");
        assert_eq!(created.overview_info_pane_id, "w7:p1");
        assert_eq!(created.overview_primary_pane_id, "w7:p2");
        assert_eq!(created.repo_tabs[0].tab_id, "w7:t2");
        assert_eq!(created.repo_tabs[0].primary_pane_id, "w7:p3");
        assert_eq!(created.agent_pane_id.as_deref(), Some("w7:p3"));

        let calls = runner.take_calls();
        assert_eq!(
            calls[1].args,
            vec!["--session", "agents", "tab", "rename", "w7:t1", "overview"]
        );
        assert_eq!(
            calls[2].args,
            vec![
                "--session",
                "agents",
                "pane",
                "split",
                "w7:p1",
                "--direction",
                "right",
                "--ratio",
                "0.5",
                "--cwd",
                "/tmp/rare-moon",
                "--no-focus",
            ]
        );
        assert_eq!(
            calls[3].args,
            vec![
                "--session",
                "agents",
                "pane",
                "run",
                "w7:p1",
                "zootree info rare-moon --watch",
            ]
        );
        assert_eq!(
            calls[4].args,
            vec![
                "--session",
                "agents",
                "tab",
                "create",
                "--workspace",
                "w7",
                "--cwd",
                "/tmp/rare-moon/zootree",
                "--label",
                "zootree",
                "--no-focus",
            ]
        );
        assert_eq!(
            calls[5].args[5..9],
            ["--direction", "right", "--ratio", "0.5"]
        );
        assert_eq!(
            calls[6].args[5..9],
            ["--direction", "down", "--ratio", "0.5"]
        );
        assert_eq!(
            calls[7].args,
            vec![
                "--session",
                "agents",
                "pane",
                "run",
                "w7:p3",
                "codex -- 'Support Herdr'",
            ]
        );
    }

    #[test]
    fn multi_repo_environment_routes_one_agent_to_overview_primary() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#,
            b"",
        ));
        runner.push_response(output(
            0,
            br#"{"result":{"type":"tab_info","tab":{"tab_id":"w7:t1"}}}"#,
            b"",
        ));
        runner.push_response(output(
            0,
            br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p2"}}}"#,
            b"",
        ));
        runner.push_response(output(0, b"", b""));
        for (tab, root, right, bottom) in [
            ("w7:t2", "w7:p3", "w7:p4", "w7:p5"),
            ("w7:t3", "w7:p6", "w7:p7", "w7:p8"),
        ] {
            runner.push_response(output(
                0,
                format!(
                    r#"{{"result":{{"type":"tab_created","tab":{{"tab_id":"{tab}"}},"root_pane":{{"pane_id":"{root}"}}}}}}"#
                )
                .as_bytes(),
                b"",
            ));
            runner.push_response(output(
                0,
                format!(r#"{{"result":{{"type":"pane_info","pane":{{"pane_id":"{right}"}}}}}}"#)
                    .as_bytes(),
                b"",
            ));
            runner.push_response(output(
                0,
                format!(r#"{{"result":{{"type":"pane_info","pane":{{"pane_id":"{bottom}"}}}}}}"#)
                    .as_bytes(),
                b"",
            ));
        }
        runner.push_response(output(0, b"", b""));

        let created = HerdrCommands::new(&runner)
            .create_environment(&EnvironmentSpec {
                session: "agents".into(),
                label: "Support Herdr".into(),
                workspace_cwd: "/tmp/rare-moon".into(),
                info_command: "zootree info rare-moon --watch".into(),
                repos: vec![
                    RepoSpec {
                        name: "api".into(),
                        cwd: "/tmp/rare-moon/api".into(),
                    },
                    RepoSpec {
                        name: "web".into(),
                        cwd: "/tmp/rare-moon/web".into(),
                    },
                ],
                agent_command: Some("codex -- 'Support Herdr'".into()),
            })
            .unwrap();

        assert_eq!(created.agent_pane_id.as_deref(), Some("w7:p2"));
        assert_eq!(created.repo_tabs.len(), 2);
        let calls = runner.take_calls();
        assert_eq!(
            calls.last().unwrap().args,
            vec![
                "--session",
                "agents",
                "pane",
                "run",
                "w7:p2",
                "codex -- 'Support Herdr'",
            ]
        );
    }

    #[test]
    fn named_session_socket_is_read_from_session_list_json() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"sessions":[{"name":"default","running":true,"socket_path":"/tmp/default.sock"},{"name":"agents","running":true,"socket_path":"/tmp/agents.sock"}]}"#,
            b"",
        ));

        let socket = HerdrCommands::new(&runner)
            .session_socket("agents")
            .unwrap();

        assert_eq!(socket.as_deref(), Some("/tmp/agents.sock"));
        assert_eq!(
            runner.take_calls()[0].args,
            vec!["session", "list", "--json"]
        );
    }

    #[test]
    fn focus_right_decodes_the_directional_focus_result() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"pane_focus_direction","focus":{"changed":true,"source_pane_id":"w7:p1","focused_pane_id":"w7:p2","reason":null,"layout":{"workspace_id":"w7","tab_id":"w7:t1","zoomed":false,"area":{"x":0,"y":0,"width":160,"height":48},"focused_pane_id":"w7:p2","panes":[],"splits":[]}}}}"#,
            b"",
        ));

        HerdrCommands::new(&runner)
            .focus_right_from("agents", "w7:p1", "w7:p2")
            .unwrap();

        assert_eq!(
            runner.take_calls()[0].args,
            vec![
                "--session",
                "agents",
                "pane",
                "focus",
                "--pane",
                "w7:p1",
                "--direction",
                "right",
            ]
        );
    }

    #[test]
    fn focus_right_reports_when_there_is_no_neighbor() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"pane_focus_direction","focus":{"changed":false,"source_pane_id":"w7:p1","focused_pane_id":null,"reason":"no_neighbor","layout":{"workspace_id":"w7","tab_id":"w7:t1","zoomed":false,"area":{"x":0,"y":0,"width":160,"height":48},"focused_pane_id":"w7:p1","panes":[],"splits":[]}}}}"#,
            b"",
        ));

        let error = HerdrCommands::new(&runner)
            .focus_right_from("agents", "w7:p1", "w7:p2")
            .unwrap_err();

        assert!(error.to_string().contains("no_neighbor"));
    }

    #[test]
    fn focus_right_rejects_an_unexpected_neighbor() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"pane_focus_direction","focus":{"changed":true,"source_pane_id":"w7:p1","focused_pane_id":"w7:p3","reason":null,"layout":{"workspace_id":"w7","tab_id":"w7:t1","zoomed":false,"area":{"x":0,"y":0,"width":160,"height":48},"focused_pane_id":"w7:p3","panes":[],"splits":[]}}}}"#,
            b"",
        ));

        let error = HerdrCommands::new(&runner)
            .focus_right_from("agents", "w7:p1", "w7:p2")
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("focused pane 'w7:p3' instead of 'w7:p2'"));
    }

    #[test]
    fn stopped_sessions_without_a_socket_do_not_break_session_listing() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"sessions":[{"name":"stopped","running":false},{"name":"agents","running":true,"socket_path":"/tmp/agents.sock"}]}"#,
            b"",
        ));

        let socket = HerdrCommands::new(&runner)
            .session_socket("agents")
            .unwrap();

        assert_eq!(socket.as_deref(), Some("/tmp/agents.sock"));
    }

    #[test]
    fn unavailable_named_session_returns_an_actionable_error() {
        let runner = MockRunner::new();
        runner.push_response(output(
            1,
            b"",
            br#"{"error":{"code":"server_not_running","message":"server is not running"}}"#,
        ));

        let error = HerdrCommands::new(&runner)
            .list_workspaces("agents")
            .unwrap_err();

        assert!(error.to_string().contains("server_not_running"));
        assert!(error.to_string().contains("herdr session attach agents"));
    }

    #[test]
    fn structural_creation_failure_closes_the_new_workspace() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#,
            b"",
        ));
        runner.push_response(output(
            1,
            b"",
            br#"{"error":{"code":"invalid_request","message":"rename failed"}}"#,
        ));
        runner.push_response(output(0, br#"{"result":{"type":"ok"}}"#, b""));

        let error = HerdrCommands::new(&runner)
            .create_environment(&EnvironmentSpec {
                session: "agents".into(),
                label: "Support Herdr".into(),
                workspace_cwd: "/tmp/rare-moon".into(),
                info_command: "zootree info rare-moon --watch".into(),
                repos: vec![RepoSpec {
                    name: "zootree".into(),
                    cwd: "/tmp/rare-moon/zootree".into(),
                }],
                agent_command: None,
            })
            .unwrap_err();

        assert!(error.to_string().contains("rename failed"));
        let calls = runner.take_calls();
        assert_eq!(
            calls.last().unwrap().args,
            vec!["--session", "agents", "workspace", "close", "w7"]
        );
    }

    #[test]
    fn incomplete_create_response_uses_the_returned_workspace_id_for_rollback() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr"}}}"#,
            b"",
        ));
        runner.push_response(output(0, br#"{"result":{"type":"ok"}}"#, b""));

        let error = HerdrCommands::new(&runner)
            .create_environment(&EnvironmentSpec {
                session: "agents".into(),
                label: "Support Herdr".into(),
                workspace_cwd: "/tmp/rare-moon".into(),
                info_command: "zootree info rare-moon --watch".into(),
                repos: vec![RepoSpec {
                    name: "zootree".into(),
                    cwd: "/tmp/rare-moon/zootree".into(),
                }],
                agent_command: None,
            })
            .unwrap_err();

        assert!(error.to_string().contains("malformed JSON"));
        assert_eq!(
            runner.take_calls().last().unwrap().args,
            vec!["--session", "agents", "workspace", "close", "w7"]
        );
    }

    #[test]
    fn completely_malformed_create_response_recovers_by_exact_label_for_rollback() {
        let runner = MockRunner::new();
        runner.push_response(output(0, b"{truncated", b""));
        runner.push_response(output(
            0,
            br#"{"result":{"type":"workspace_list","workspaces":[{"workspace_id":"w7","label":"Support Herdr"}]}}"#,
            b"",
        ));
        runner.push_response(output(0, br#"{"result":{"type":"ok"}}"#, b""));

        let error = HerdrCommands::new(&runner)
            .create_environment(&EnvironmentSpec {
                session: "agents".into(),
                label: "Support Herdr".into(),
                workspace_cwd: "/tmp/rare-moon".into(),
                info_command: "zootree info rare-moon --watch".into(),
                repos: vec![RepoSpec {
                    name: "zootree".into(),
                    cwd: "/tmp/rare-moon/zootree".into(),
                }],
                agent_command: None,
            })
            .unwrap_err();

        assert!(error.to_string().contains("malformed JSON"));
        let calls = runner.take_calls();
        assert_eq!(
            calls[1].args,
            vec!["--session", "agents", "workspace", "list"]
        );
        assert_eq!(
            calls[2].args,
            vec!["--session", "agents", "workspace", "close", "w7"]
        );
    }

    #[test]
    fn malformed_success_response_is_a_creation_failure_and_rolls_back() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#,
            b"",
        ));
        runner.push_response(output(0, br#"{"result":{}}"#, b""));
        runner.push_response(output(0, br#"{"result":{"type":"ok"}}"#, b""));

        let error = HerdrCommands::new(&runner)
            .create_environment(&EnvironmentSpec {
                session: "agents".into(),
                label: "Support Herdr".into(),
                workspace_cwd: "/tmp/rare-moon".into(),
                info_command: "zootree info rare-moon --watch".into(),
                repos: vec![RepoSpec {
                    name: "zootree".into(),
                    cwd: "/tmp/rare-moon/zootree".into(),
                }],
                agent_command: None,
            })
            .unwrap_err();

        assert!(error.to_string().contains("malformed JSON"));
        assert_eq!(
            runner.take_calls().last().unwrap().args,
            vec!["--session", "agents", "workspace", "close", "w7"]
        );
    }

    #[test]
    fn rollback_failure_is_attached_to_the_original_creation_error() {
        let runner = MockRunner::new();
        runner.push_response(output(
            0,
            br#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#,
            b"",
        ));
        runner.push_response(output(
            1,
            b"",
            br#"{"error":{"code":"invalid_request","message":"rename failed"}}"#,
        ));
        runner.push_response(output(
            1,
            b"",
            br#"{"error":{"code":"internal","message":"close failed"}}"#,
        ));

        let error = HerdrCommands::new(&runner)
            .create_environment(&EnvironmentSpec {
                session: "agents".into(),
                label: "Support Herdr".into(),
                workspace_cwd: "/tmp/rare-moon".into(),
                info_command: "zootree info rare-moon --watch".into(),
                repos: vec![RepoSpec {
                    name: "zootree".into(),
                    cwd: "/tmp/rare-moon/zootree".into(),
                }],
                agent_command: None,
            })
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("rename failed"));
        assert!(message.contains("additionally failed to roll back"));
        assert!(message.contains("close failed"));
    }

    #[test]
    fn every_post_create_transaction_failure_rolls_back_the_workspace() {
        for failed_step in 1..=7 {
            let runner = MockRunner::new();
            let responses: Vec<Output> = vec![
                output(
                    0,
                    br#"{"result":{"type":"workspace_created","workspace":{"workspace_id":"w7","label":"Support Herdr"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#,
                    b"",
                ),
                output(0, br#"{"result":{"type":"tab_info","tab":{"tab_id":"w7:t1"}}}"#, b""),
                output(0, br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p2"}}}"#, b""),
                output(0, b"", b""),
                output(0, br#"{"result":{"type":"tab_created","tab":{"tab_id":"w7:t2"},"root_pane":{"pane_id":"w7:p3"}}}"#, b""),
                output(0, br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p4"}}}"#, b""),
                output(0, br#"{"result":{"type":"pane_info","pane":{"pane_id":"w7:p5"}}}"#, b""),
                output(0, b"", b""),
            ];
            for (index, response) in responses.into_iter().enumerate() {
                if index == failed_step {
                    runner.push_response(output(
                        1,
                        b"",
                        br#"{"error":{"code":"injected","message":"step failed"}}"#,
                    ));
                    break;
                }
                runner.push_response(response);
            }
            runner.push_response(output(0, br#"{"result":{"type":"ok"}}"#, b""));

            let error = HerdrCommands::new(&runner)
                .create_environment(&EnvironmentSpec {
                    session: "agents".into(),
                    label: "Support Herdr".into(),
                    workspace_cwd: "/tmp/rare-moon".into(),
                    info_command: "zootree info rare-moon --watch".into(),
                    repos: vec![RepoSpec {
                        name: "zootree".into(),
                        cwd: "/tmp/rare-moon/zootree".into(),
                    }],
                    agent_command: Some("codex -- 'Support Herdr'".into()),
                })
                .unwrap_err();

            assert!(error.to_string().contains("step failed"));
            assert_eq!(
                runner.take_calls().last().unwrap().args,
                vec!["--session", "agents", "workspace", "close", "w7"],
                "failed step {failed_step} should roll back"
            );
        }
    }
}
