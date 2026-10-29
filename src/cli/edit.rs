use std::ffi::OsStr;

use anyhow::Result;
use clap::Args;
use clap_complete::ArgValueCompleter;

use crate::config::workspace::WorkspaceStatus;
use crate::config::ConfigManager;
use crate::core::completers::{complete_workspace, WorkspaceFilter};
use crate::core::editor;
use crate::tui;
use crate::tui_app::info::status_label;

#[derive(Args, Debug)]
pub struct EditArgs {
    #[arg(
        help = "Workspace name (interactive if omitted)",
        add = ArgValueCompleter::new(|c: &OsStr| complete_workspace(c, WorkspaceFilter::Any))
    )]
    pub name: Option<String>,

    #[arg(
        long,
        requires = "name",
        help = "Print the Workspace configuration path instead of opening an editor"
    )]
    pub path: bool,
}

pub fn handle_edit(args: &EditArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            let all = config_mgr.list_workspaces_with_status(None)?;
            if all.is_empty() {
                anyhow::bail!("no workspaces found");
            }
            let items: Vec<String> = all
                .iter()
                .map(|w| {
                    format!(
                        "{} - {} [{}]",
                        w.config.name,
                        w.config.title,
                        status_label(&w.status)
                    )
                })
                .collect();
            let idx = tui::select_one("Select workspace to edit", &items)?;
            all[idx].config.name.clone()
        }
    };

    let (status, path) = config_mgr.locate_workspace_config(&name)?;

    if args.path {
        println!("{}", path.display());
        return Ok(());
    }

    if matches!(status, WorkspaceStatus::Done | WorkspaceStatus::Canceled) {
        eprintln!(
            "note: workspace '{}' is archived ({}); edits may affect `zootree reopen`",
            name,
            status_label(&status)
        );
    }

    editor::open_file(&path)?;
    config_mgr.check_edited_workspace_config(&name, &path)?;
    Ok(())
}
