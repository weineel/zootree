use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::TempDir;
use zootree::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;

fn workspace(name: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Title".into(),
        name: name.into(),
        description: String::new(),
        branch: name.into(),
        workspace_dir: format!("/tmp/{name}"),
        created_at: "2026-01-01T00:00:00Z".into(),
        agent_cli: None,
        multiplexer: Default::default(),
        multiplexer_state: Default::default(),
        repos: vec![],
        events: vec![],
    }
}

fn manager(home: &TempDir) -> ConfigManager {
    let mgr = ConfigManager::with_base_dir(home.path().join(".config/zootree"));
    mgr.ensure_dirs().unwrap();
    mgr
}

fn config_path(home: &TempDir, status_dir: &str, name: &str) -> PathBuf {
    home.path()
        .join(".config/zootree/workspaces")
        .join(status_dir)
        .join(format!("{name}.toml"))
}

fn editor(home: &TempDir, body: &str) -> PathBuf {
    let path = home.path().join("editor.sh");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

fn run(home: &TempDir, args: &[&str], visual: Option<&PathBuf>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zootree"));
    cmd.env("HOME", home.path()).env_remove("EDITOR").args(args);
    match visual {
        Some(v) => cmd.env("VISUAL", v),
        None => cmd.env_remove("VISUAL"),
    };
    cmd.output().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn path_prints_only_the_config_path_for_every_status() {
    let home = TempDir::new().unwrap();
    let mgr = manager(&home);
    for (status, dir, name) in [
        (WorkspaceStatus::Pending, "pending", "w-pending"),
        (WorkspaceStatus::InProgress, "in_progress", "w-prog"),
        (WorkspaceStatus::Done, "archived/done", "w-done"),
        (WorkspaceStatus::Canceled, "archived/canceled", "w-cancel"),
    ] {
        mgr.save_workspace(&status, &workspace(name)).unwrap();
        let out = run(&home, &["edit", name, "--path"], None);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        assert!(!stderr(&out).contains("reopen"), "{}", stderr(&out));
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            format!("{}\n", config_path(&home, dir, name).display())
        );
    }
}

#[test]
fn path_fails_for_unknown_workspace() {
    let home = TempDir::new().unwrap();
    manager(&home);
    let out = run(&home, &["edit", "nope", "--path"], None);
    assert!(!out.status.success());
}

#[test]
fn edit_opens_the_config_in_the_editor_and_keeps_valid_changes() {
    let home = TempDir::new().unwrap();
    manager(&home)
        .save_workspace(&WorkspaceStatus::InProgress, &workspace("w1"))
        .unwrap();
    let ed = editor(
        &home,
        "sed -i.bak 's/^title = .*/title = \"Edited\"/' \"$1\"",
    );

    let out = run(&home, &["edit", "w1"], Some(&ed));

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let content = std::fs::read_to_string(config_path(&home, "in_progress", "w1")).unwrap();
    assert!(content.contains("title = \"Edited\""), "{content}");
}

#[test]
fn edit_warns_about_archived_workspaces() {
    let home = TempDir::new().unwrap();
    manager(&home)
        .save_workspace(&WorkspaceStatus::Done, &workspace("old"))
        .unwrap();
    let ed = editor(&home, "true");

    let out = run(&home, &["edit", "old"], Some(&ed));

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains("reopen"), "stderr: {}", stderr(&out));
}

#[test]
fn edit_reports_invalid_toml_without_reverting() {
    let home = TempDir::new().unwrap();
    manager(&home)
        .save_workspace(&WorkspaceStatus::Pending, &workspace("w2"))
        .unwrap();
    let ed = editor(&home, "echo 'broken = [' >> \"$1\"");

    let out = run(&home, &["edit", "w2"], Some(&ed));

    assert!(!out.status.success());
    let content = std::fs::read_to_string(config_path(&home, "pending", "w2")).unwrap();
    assert!(content.contains("broken = ["), "{content}");
}

#[test]
fn edit_reports_name_mismatch_with_expected_name() {
    let home = TempDir::new().unwrap();
    manager(&home)
        .save_workspace(&WorkspaceStatus::Pending, &workspace("w3"))
        .unwrap();
    let ed = editor(&home, "sed -i.bak 's/^name = .*/name = \"other\"/' \"$1\"");

    let out = run(&home, &["edit", "w3"], Some(&ed));

    assert!(!out.status.success());
    // Errors are logged through tracing, whose terminal layer writes to stdout.
    let output = format!("{}{}", String::from_utf8_lossy(&out.stdout), stderr(&out));
    assert!(output.contains("must be 'w3'"), "output: {output}");
    let content = std::fs::read_to_string(config_path(&home, "pending", "w3")).unwrap();
    assert!(content.contains("name = \"other\""), "{content}");
}
