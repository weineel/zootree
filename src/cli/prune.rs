use clap::Args;
use crate::config::ConfigManager;
use crate::config::workspace::WorkspaceStatus;
use crate::tui;
use anyhow::Result;
use std::path::Path;

#[derive(Args)]
pub struct PruneArgs {
    #[arg(long)]
    pub all: bool,
}

pub fn handle_prune(args: &PruneArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let mut archived = Vec::new();
    for status in [WorkspaceStatus::Done, WorkspaceStatus::Canceled] {
        let workspaces = config_mgr.list_workspaces(Some(&[status.clone()]))?;
        for ws in workspaces {
            archived.push((status.clone(), ws));
        }
    }

    if archived.is_empty() {
        println!("no archived workspaces to prune");
        return Ok(());
    }

    let to_prune = if args.all {
        archived
    } else {
        let names: Vec<String> = archived.iter()
            .map(|(s, w)| format!("{} ({:?})", w.name, s))
            .collect();
        let selected = tui::select_multi("Select workspaces to prune", &names)?;
        selected.into_iter().map(|i| archived[i].clone()).collect()
    };

    if to_prune.is_empty() {
        println!("nothing selected");
        return Ok(());
    }

    for (status, ws) in &to_prune {
        let ws_dir = shellexpand::tilde(&ws.workspace_dir).into_owned();

        if Path::new(&ws_dir).exists() {
            std::fs::remove_dir_all(&ws_dir)?;
            println!("  removed directory: {}", ws_dir);
        }

        config_mgr.delete_workspace_config(&ws.name, status)?;
        println!("  pruned: {}", ws.name);
    }

    println!("{} workspace(s) pruned", to_prune.len());
    Ok(())
}
