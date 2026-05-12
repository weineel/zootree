use std::ffi::OsStr;
use std::fmt::Write as _;

use anyhow::Result;
use clap::Args;
use clap_complete::ArgValueCompleter;

use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::core::completers::{complete_workspace, WorkspaceFilter};
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
        // Filled in by Task 8.
        anyhow::bail!("--watch not implemented yet");
    }

    print!("{}", render_once(&status, &workspace));
    Ok(())
}

/// Build the multi-line textual report shown by `zootree info <name>`
/// without `--watch`. Pure function — easy to test.
pub fn render_once(status: &WorkspaceStatus, ws: &WorkspaceConfig) -> String {
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
    let _ = writeln!(out, "Repos:");
    if ws.repos.is_empty() {
        let _ = writeln!(out, "  (none)");
    } else {
        for r in &ws.repos {
            let target = r.target_branch.as_deref().unwrap_or("*");
            let _ = writeln!(out, "  - {:<15} -> {}", r.name, target);
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
