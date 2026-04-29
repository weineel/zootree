use clap::Args;

#[derive(Args)]
pub struct CreateArgs {
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub repos: Option<String>,
    #[arg(long)]
    pub branch: Option<String>,
    #[arg(long)]
    pub template: Option<String>,
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long)]
    pub status: Option<String>,
}

#[derive(Args)]
pub struct StartArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub no_zellij: bool,
}

#[derive(Args)]
pub struct OpenArgs {
    pub name: Option<String>,
}

#[derive(Args)]
pub struct DoneArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub no_merge: bool,
    #[arg(long)]
    pub no_clean: bool,
    #[arg(long)]
    pub push: bool,
    #[arg(long)]
    pub delete_remote: bool,
    #[arg(long)]
    pub strategy: Option<String>,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct CancelArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub no_clean: bool,
    #[arg(long)]
    pub force: bool,
}
