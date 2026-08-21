use crate::runner::{CommandRunner, CommandSpec, RealRunner};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

pub fn open_file(path: &Path) -> Result<()> {
    let visual = std::env::var("VISUAL").ok();
    let editor = std::env::var("EDITOR").ok();
    let runner = RealRunner;
    open_file_with(path, &runner, visual.as_deref(), editor.as_deref())
}

pub fn open_file_with<R: CommandRunner>(
    path: &Path,
    runner: &R,
    visual: Option<&str>,
    editor: Option<&str>,
) -> Result<()> {
    let command = visual
        .filter(|value| !value.trim().is_empty())
        .or_else(|| editor.filter(|value| !value.trim().is_empty()))
        .unwrap_or("vi");
    let mut parts = shlex::split(command)
        .ok_or_else(|| anyhow::anyhow!("invalid editor command: {command}"))?;
    if parts.is_empty() {
        anyhow::bail!("editor command cannot be empty");
    }
    let program = parts.remove(0);
    parts.push(path.to_string_lossy().into_owned());

    let spec = CommandSpec {
        program,
        args: parts,
        cwd: None,
        env: HashMap::new(),
        env_remove: Vec::new(),
    };
    let status = runner.run_interactive(&spec)?;
    if !status.success() {
        anyhow::bail!("editor '{}' exited with {}", spec.program, status);
    }
    Ok(())
}
