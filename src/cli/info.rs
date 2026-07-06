use std::ffi::OsStr;
use std::fmt::Write as _;

use anyhow::Result;
use clap::Args;
use clap_complete::ArgValueCompleter;

use crate::config::global::GlobalConfig;
use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::completers::{complete_workspace, WorkspaceFilter};
use crate::core::repo_status::missing_registered_repo_names;
use crate::core::worktree_status::repo_worktree_statuses;
use crate::tui;
use crate::tui_app::info::{format_rfc3339_to_minute, last_n, status_label};

#[derive(Args, Debug)]
pub struct InfoArgs {
    #[arg(
        help = "Workspace name (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &OsStr| complete_workspace(c, WorkspaceFilter::Any))
    )]
    pub name: Option<String>,

    #[arg(long, help = "Watch mode: render as a TUI and auto-refresh")]
    pub watch: bool,

    #[arg(
        long,
        default_value = "5",
        help = "Refresh interval in seconds (used with --watch)"
    )]
    pub interval: u64,
}

pub fn handle_info(args: &InfoArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let all = config_mgr.list_workspaces(None)?;
            if all.is_empty() {
                anyhow::bail!("no workspaces found");
            }
            let items: Vec<String> = all
                .iter()
                .map(|w| format!("{} - {}", w.name, w.title))
                .collect();
            let idx = tui::select_one("Select workspace", &items)?;
            all[idx].name.clone()
        }
    };

    let (status, workspace) = config_mgr.load_workspace(&name)?;

    if args.watch {
        // `workspace` / `status` above were just a reachability check; the TUI
        // reloads on its own via the consumed config_mgr.
        let _ = (status, workspace);
        let app = crate::tui_app::info::InfoApp::new(
            name,
            config_mgr,
            true,
            std::time::Duration::from_secs(args.interval),
        );
        crate::tui_app::run_app(app)?;
        return Ok(());
    }

    let global = config_mgr.load_global_config().unwrap_or_default();
    let missing_repos = missing_registered_repo_names(&config_mgr, &workspace.repos);
    print!(
        "{}",
        render_once_with_missing_repos(&status, &workspace, &global, &missing_repos)
    );
    Ok(())
}

/// Build the multi-line textual report shown by `zootree info <name>`
/// without `--watch`. Pure function — easy to test.
pub fn render_once(
    status: &WorkspaceStatus,
    ws: &WorkspaceConfig,
    global: &GlobalConfig,
) -> String {
    render_once_with_missing_repos(status, ws, global, &[])
}

pub fn render_once_with_missing_repos(
    status: &WorkspaceStatus,
    ws: &WorkspaceConfig,
    global: &GlobalConfig,
    missing_repos: &[String],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Workspace: {} ({})", ws.title, ws.name);
    let _ = writeln!(out, "Status:    {}", status_label(status));
    let _ = writeln!(out, "Branch:    {}", ws.branch);
    let _ = writeln!(out, "Dir:       {}", ws.workspace_dir);
    let _ = writeln!(
        out,
        "Created:   {}",
        format_rfc3339_to_minute(&ws.created_at)
    );
    if !ws.description.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Description:");
        for l in ws.description.lines() {
            let _ = writeln!(out, "  {}", l);
        }
    }
    let _ = writeln!(out);
    let agent_cli = ws.agent_cli.as_deref().or(global.agent_cli.as_deref());
    match crate::core::layout::build_agent_cli_display(agent_cli, &global.agent_cli_alias, ws) {
        Some(Ok(display)) => {
            let _ = writeln!(out, "Agent:");
            if let Some(alias) = &display.alias {
                let _ = writeln!(out, "  {}  (via alias: {})", display.command, alias.name);
                let _ = writeln!(out);
                let _ = writeln!(out, "Alias:");
                let _ = writeln!(out, "  {} = {}", alias.name, alias.template);
            } else {
                let _ = writeln!(out, "  {}", display.command);
            }
        }
        Some(Err(e)) => {
            let _ = writeln!(out, "Agent:");
            let _ = writeln!(out, "  (failed to parse agent_cli: {:#})", e);
        }
        None => {
            let _ = writeln!(out, "Prompt:");
            for l in crate::core::layout::build_prompt(ws).lines() {
                let _ = writeln!(out, "  {}", l);
            }
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "Repos:");
    if ws.repos.is_empty() {
        let _ = writeln!(out, "  (none)");
    } else {
        let worktrees = if matches!(status, WorkspaceStatus::InProgress) {
            let ws_dir = shellexpand::tilde(&ws.workspace_dir).into_owned();
            repo_worktree_statuses(ws, &ws_dir)
        } else {
            Vec::new()
        };

        for r in &ws.repos {
            let target = r.target_branch.as_deref().unwrap_or("*");
            let repo_missing = missing_repos.contains(&r.name);
            if let Some(worktree) = worktrees.iter().find(|status| status.repo_name == r.name) {
                let missing = if worktree.exists && !repo_missing {
                    ""
                } else {
                    " (missing)"
                };
                let _ = writeln!(
                    out,
                    "  - {:<15} -> {}  {}{}",
                    r.name, target, worktree.worktree_path, missing
                );
            } else {
                let missing = if repo_missing { " (missing)" } else { "" };
                let _ = writeln!(out, "  - {:<15} -> {}{}", r.name, target, missing);
            }
        }
    }
    if !ws.events.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Recent events:");
        for e in last_n(&ws.events, 5) {
            let ts = format_rfc3339_to_minute(&e.timestamp);
            if let Some(d) = &e.detail {
                let _ = writeln!(out, "  {}  {}  ({})", ts, e.action, d);
            } else {
                let _ = writeln!(out, "  {}  {}", ts, e.action);
            }
        }
    }
    out
}
