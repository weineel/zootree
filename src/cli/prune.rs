use crate::cli::remote::{self, DaemonClient, ExecOptions};
use crate::cli::workspace::CliEventSink;
use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;
use crate::service::prune::{archived_workspaces, prune_workspaces, PruneRequest, PruneTarget};
use crate::service::{EnvSnapshot, RequestContext};
use crate::tui;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PruneArgs {
    #[arg(long, help = "Prune all archived workspaces without prompting")]
    pub all: bool,
}

pub fn handle_prune(args: &PruneArgs) -> Result<()> {
    let config_mgr = ConfigManager::new()?;

    let archived = archived_workspaces(&config_mgr)?;
    // With nothing archived the service reports it; no prompt is needed.
    let request = if args.all || archived.is_empty() {
        PruneRequest {
            all: true,
            targets: Vec::new(),
        }
    } else {
        let names: Vec<String> = archived
            .iter()
            .map(|(s, w)| format!("{} ({:?})", w.name, s))
            .collect();
        let selected = tui::select_multi("Select workspaces to prune", &names)?;
        PruneRequest {
            all: false,
            targets: selected
                .into_iter()
                .map(|i| PruneTarget {
                    name: archived[i].1.name.clone(),
                    status: archived[i].0.clone(),
                })
                .collect(),
        }
    };

    if let Some(daemon) = remote::connect(&config_mgr) {
        if prune_through_daemon(&daemon, &archived, &request)? {
            return Ok(());
        }
    }

    let sink = CliEventSink;
    let ctx = RequestContext::new(&sink, EnvSnapshot::capture());
    prune_workspaces(&config_mgr, &ctx, &request)?;
    Ok(())
}

/// Prunes through the Daemon, one Operation per Workspace. Each prints its own
/// closing count, which is dropped; the total is printed once at the end, as
/// the local service does. `false`: the Daemon could not take the first
/// request, so everything still has to be done locally.
fn prune_through_daemon(
    daemon: &DaemonClient,
    archived: &[(WorkspaceStatus, WorkspaceConfig)],
    request: &PruneRequest,
) -> Result<bool> {
    if archived.is_empty() {
        println!("no archived workspaces to prune");
        return Ok(true);
    }
    let names: Vec<String> = if request.all {
        archived.iter().map(|(_, ws)| ws.name.clone()).collect()
    } else {
        // The local service rejects a target that is not archived.
        for target in &request.targets {
            if !archived
                .iter()
                .any(|(status, ws)| *status == target.status && ws.name == target.name)
            {
                anyhow::bail!("archived workspace '{}' not found", target.name);
            }
        }
        request.targets.iter().map(|t| t.name.clone()).collect()
    };
    if names.is_empty() {
        println!("nothing selected");
        return Ok(true);
    }
    let options = ExecOptions {
        skip_step: Some(is_prune_total),
        ..Default::default()
    };
    for (index, name) in names.iter().enumerate() {
        let finished = daemon.execute(
            "DELETE",
            &format!("/workspaces/{name}"),
            serde_json::json!({}),
            &options,
        )?;
        if finished.is_none() {
            if index == 0 {
                return Ok(false);
            }
            anyhow::bail!("lost the zootree daemon after pruning {index} workspace(s)");
        }
    }
    println!("{} workspace(s) pruned", names.len());
    Ok(true)
}

fn is_prune_total(message: &str) -> bool {
    message.ends_with(" workspace(s) pruned")
        && message.chars().next().is_some_and(|c| c.is_ascii_digit())
}
