use crate::cli::workspace::CliEventSink;
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

    let sink = CliEventSink;
    let ctx = RequestContext::new(&sink, EnvSnapshot::capture());
    prune_workspaces(&config_mgr, &ctx, &request)?;
    Ok(())
}
