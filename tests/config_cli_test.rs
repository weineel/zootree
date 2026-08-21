use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::process::{Command, Output};
use tempfile::TempDir;
use zootree::config::{global::GlobalConfig, ConfigManager};

fn configured_agents() -> GlobalConfig {
    GlobalConfig {
        agent_cli: Some("codex".into()),
        agent_cli_alias: BTreeMap::from([
            ("claude".into(), "claude -- $prompt".into()),
            (
                "codex".into(),
                "codex --ask-for-approval never -- $prompt".into(),
            ),
            ("gemini".into(), "gemini --prompt $prompt".into()),
        ]),
        ..Default::default()
    }
}

fn write_global_config(home: &TempDir, config: &GlobalConfig) {
    let manager = ConfigManager::with_base_dir(home.path().join(".config/zootree"));
    manager.ensure_dirs().unwrap();
    manager.save_global_config(config).unwrap();
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
    write_global_config(&home, &configured_agents());

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
    write_global_config(&home, &configured_agents());

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
            "  codex (default) -> codex --ask-for-approval never -- $prompt\n",
            "  claude -> claude -- $prompt\n",
            "  gemini -> gemini --prompt $prompt\n",
        )
    );
}
