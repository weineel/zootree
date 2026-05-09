pub mod prune;
pub mod repo;
pub mod template;
pub mod workspace;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "zootree",
    about = "Multi-repo collaborative workspace manager",
    version,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(long, global = true, help = "Enable verbose logging output")]
    pub verbose: bool,

    #[arg(long, global = true, help = "Suppress all output except errors")]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "Manage registered repositories")]
    Repo(repo::RepoArgs),
    #[command(about = "Create a new workspace")]
    Create(workspace::CreateArgs),
    #[command(about = "List workspaces")]
    List(workspace::ListArgs),
    #[command(about = "Start a pending workspace (create worktrees and launch zellij)")]
    Start(workspace::StartArgs),
    #[command(about = "Open an in-progress workspace in zellij")]
    Open(workspace::OpenArgs),
    #[command(about = "Complete a workspace (merge, clean up worktrees)")]
    Done(workspace::DoneArgs),
    #[command(about = "Cancel a workspace (discard worktrees without merging)")]
    Cancel(workspace::CancelArgs),
    #[command(about = "Manage workspace templates")]
    Template(template::TemplateArgs),
    #[command(about = "Remove archived workspace directories and configs")]
    Prune(prune::PruneArgs),
    #[command(about = "Show log file location")]
    Logs,
}
