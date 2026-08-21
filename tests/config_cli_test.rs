use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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

fn run_zootree_with_visual(home: &TempDir, args: &[&str], visual: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zootree"))
        .env("HOME", home.path())
        .env("VISUAL", visual)
        .env_remove("EDITOR")
        .args(args)
        .output()
        .unwrap()
}

fn write_editor_script(home: &TempDir, body: &str) -> PathBuf {
    let path = home.path().join("editor.sh");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

#[test]
fn config_path_prints_the_global_config_file_without_creating_it() {
    let home = TempDir::new().unwrap();

    let output = run_zootree(&home, &["config", "path"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "{}\n",
            home.path().join(".config/zootree/config.toml").display()
        )
    );
    assert!(!home.path().join(".config").exists());
}

#[test]
fn config_path_is_absolute_when_home_is_relative() {
    let cwd = TempDir::new().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_zootree"))
        .current_dir(cwd.path())
        .env("HOME", "relative-home")
        .args(["config", "path"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = String::from_utf8(output.stdout).unwrap();
    assert!(Path::new(printed.trim()).is_absolute(), "stdout: {printed}");
    assert!(
        printed.ends_with("/relative-home/.config/zootree/config.toml\n"),
        "stdout: {printed}"
    );
}

#[test]
fn config_show_prints_a_malformed_global_config_verbatim_without_starting_logging() {
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".config/zootree");
    std::fs::create_dir_all(&config_dir).unwrap();
    let source = b"# keep this comment\nworkspace_root = \"~/custom\"\nbroken = [\n";
    std::fs::write(config_dir.join("config.toml"), source).unwrap();

    let output = run_zootree(&home, &["config", "show"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, source);
    assert!(!config_dir.join("logs").exists());
}

#[test]
fn config_show_reports_a_missing_file_with_edit_recovery_guidance() {
    let home = TempDir::new().unwrap();
    let path = home.path().join(".config/zootree/config.toml");

    let output = run_zootree(&home, &["config", "show"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(&path.display().to_string()),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("zootree config edit"), "stderr: {stderr}");
    assert!(!home.path().join(".config").exists());
}

#[test]
fn config_edit_creates_an_empty_global_config_without_starting_logging() {
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".config/zootree");
    let config_path = config_dir.join("config.toml");

    let output = run_zootree_with_visual(&home, &["config", "edit"], "true");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(config_path).unwrap(), b"");
    assert!(!config_dir.join("logs").exists());
}

#[test]
fn config_edit_reports_invalid_toml_and_preserves_the_edited_file() {
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".config/zootree");
    let config_path = config_dir.join("config.toml");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(&config_path, "workspace_root = \"~/before\"\n").unwrap();
    let editor = write_editor_script(&home, "printf 'broken = [\\n' > \"$1\"");

    let output = run_zootree_with_visual(&home, &["config", "edit"], editor.to_str().unwrap());

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("failed to parse global config"),
        "stderr: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(config_path).unwrap(),
        "broken = [\n"
    );
}

#[test]
fn config_edit_reports_when_the_editor_deletes_the_config_file() {
    let home = TempDir::new().unwrap();
    let config_dir = home.path().join(".config/zootree");
    let config_path = config_dir.join("config.toml");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(&config_path, "workspace_root = \"~/before\"\n").unwrap();
    let editor = write_editor_script(&home, "rm \"$1\"");

    let output = run_zootree_with_visual(&home, &["config", "edit"], editor.to_str().unwrap());

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("failed to read global config"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "stderr: {stderr}"
    );
    assert!(!config_path.exists());
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
