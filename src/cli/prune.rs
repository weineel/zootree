use clap::Args;

#[derive(Args)]
pub struct PruneArgs {
    #[arg(long)]
    pub all: bool,
}
