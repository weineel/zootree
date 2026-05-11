use anyhow::{anyhow, Result};
use clap::Args;
use clap_complete::env::Shells;
use clap_complete::Shell;
use std::io::{self, Write};

#[derive(Args)]
pub struct CompletionsArgs {
    #[arg(value_enum, help = "Target shell")]
    pub shell: Shell,
}

pub fn handle_completions(args: &CompletionsArgs) -> Result<()> {
    let completer_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.into_os_string().into_string().ok())
        .unwrap_or_else(|| "zootree".to_string());
    write_registration(args.shell, &completer_path, &mut io::stdout())
}

pub fn write_registration(shell: Shell, completer_path: &str, buf: &mut dyn Write) -> Result<()> {
    let shell_name = shell.to_string();
    let shells = Shells::builtins();
    let completer = shells
        .completer(&shell_name)
        .ok_or_else(|| anyhow!("unsupported shell: {}", shell_name))?;
    completer.write_registration("COMPLETE", "zootree", "zootree", completer_path, buf)?;
    Ok(())
}
