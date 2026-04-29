use clap::{Args, Subcommand};

#[derive(Args)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub command: RepoCommands,
}

#[derive(Subcommand)]
pub enum RepoCommands {
    Add {
        name: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        default_target_branch: Option<String>,
    },
    List,
    Edit {
        name: Option<String>,
    },
    Remove {
        name: Option<String>,
    },
}
