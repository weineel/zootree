pub mod repo;
pub mod workspace;
pub mod template;
pub mod prune;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "zootree", about = "Multi-repo collaborative workspace manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(long, global = true)]
    pub verbose: bool,

    #[arg(long, global = true)]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    Repo(repo::RepoArgs),
    Create(workspace::CreateArgs),
    List(workspace::ListArgs),
    Start(workspace::StartArgs),
    Open(workspace::OpenArgs),
    Done(workspace::DoneArgs),
    Cancel(workspace::CancelArgs),
    Template(template::TemplateArgs),
    Prune(prune::PruneArgs),
    Logs,
}
