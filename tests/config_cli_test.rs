use serde_json::{json, Value};
use std::fs;
use std::process::{Command, Output};
use tempfile::TempDir;

fn write_global_config(home: &TempDir, content: &str) {
    let config_dir = home.path().join(".config/zootree");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.toml"), content).unwrap();
}

fn run_zootree(home: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zootree"))
        .env("HOME", home.path())
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn config_agents_json_lists_default_first_and_resolves_its_command() {
    let home = TempDir::new().unwrap();
    write_global_config(
        &home,
        r#"
agent_cli = "codex"

[agent_cli_alias]
claude = "claude -- $prompt"
codex = "codex --ask-for-approval never -- $prompt"
gemini = "gemini --prompt $prompt"
"#,
    );

    let output = run_zootree(&home, &["config", "agents", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        actual,
        json!({
            "default": {
                "value": "codex",
                "kind": "alias",
                "command": "codex --ask-for-approval never -- $prompt"
            },
            "aliases": [
                {
                    "name": "codex",
                    "command": "codex --ask-for-approval never -- $prompt",
                    "is_default": true
                },
                {
                    "name": "claude",
                    "command": "claude -- $prompt",
                    "is_default": false
                },
                {
                    "name": "gemini",
                    "command": "gemini --prompt $prompt",
                    "is_default": false
                }
            ]
        })
    );
}

#[test]
fn config_agents_prints_a_human_readable_default_and_choices() {
    let home = TempDir::new().unwrap();
    write_global_config(
        &home,
        r#"
agent_cli = "codex"

[agent_cli_alias]
claude = "claude -- $prompt"
codex = "codex --ask-for-approval never -- $prompt"
gemini = "gemini --prompt $prompt"
"#,
    );

    let output = run_zootree(&home, &["config", "agents"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "Default: codex (alias)\n",
            "Agents:\n",
            "  codex (default) -> codex --ask-for-approval never -- $prompt\n",
            "  claude -> claude -- $prompt\n",
            "  gemini -> gemini --prompt $prompt\n",
        )
    );
}
