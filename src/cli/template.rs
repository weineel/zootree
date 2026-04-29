use clap::{Args, Subcommand};

#[derive(Args)]
pub struct TemplateArgs {
    #[command(subcommand)]
    pub command: TemplateCommands,
}

#[derive(Subcommand)]
pub enum TemplateCommands {
    List,
    Save {
        name: String,
        #[arg(long)]
        from: String,
    },
}
