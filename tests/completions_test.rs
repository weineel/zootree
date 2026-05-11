use chrono::Local;
use std::ffi::OsStr;
use tempfile::TempDir;
use zootree::config::global::{HooksConfig, ZellijConfig};
use zootree::config::repo::RepoConfig;
use zootree::config::template::TemplateConfig;
use zootree::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use zootree::config::ConfigManager;
use zootree::core::completers::{
    complete_repo_with, complete_repos_list_with, complete_template_with, complete_workspace_with,
    WorkspaceFilter,
};

fn make_workspace(name: &str, title: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: title.into(),
        name: name.into(),
        description: String::new(),
        branch: format!("zootree/{}", name),
        workspace_dir: format!("/tmp/{}", name),
        created_at: Local::now().to_rfc3339(),
        zellij: ZellijConfig {
            session_mode: Some("standalone".into()),
            ..Default::default()
        },
        repos: Vec::new(),
        events: Vec::new(),
    }
}

fn make_mgr() -> (TempDir, ConfigManager) {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    mgr.ensure_dirs().unwrap();
    (tmp, mgr)
}

fn save(mgr: &ConfigManager, status: WorkspaceStatus, name: &str, title: &str) {
    mgr.save_workspace(&status, &make_workspace(name, title))
        .unwrap();
}

fn names(cands: &[clap_complete::CompletionCandidate]) -> Vec<String> {
    cands
        .iter()
        .map(|c| c.get_value().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn workspace_completer_filters_pending() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login");
    save(
        &mgr,
        WorkspaceStatus::InProgress,
        "add-search",
        "Add search",
    );
    save(&mgr, WorkspaceStatus::Done, "old-feat", "Old feat");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Pending);
    assert_eq!(names(&cands), vec!["fix-login"]);
}

#[test]
fn workspace_completer_filters_in_progress() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login");
    save(
        &mgr,
        WorkspaceStatus::InProgress,
        "add-search",
        "Add search",
    );
    save(&mgr, WorkspaceStatus::Done, "old-feat", "Old feat");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::InProgress);
    assert_eq!(names(&cands), vec!["add-search"]);
}

#[test]
fn workspace_completer_filters_active() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login");
    save(
        &mgr,
        WorkspaceStatus::InProgress,
        "add-search",
        "Add search",
    );
    save(&mgr, WorkspaceStatus::Done, "old-feat", "Old feat");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Active);
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["add-search", "fix-login"]);
}

#[test]
fn workspace_completer_any_includes_all() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "a", "A");
    save(&mgr, WorkspaceStatus::InProgress, "b", "B");
    save(&mgr, WorkspaceStatus::Done, "c", "C");
    save(&mgr, WorkspaceStatus::Canceled, "d", "D");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Any);
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["a", "b", "c", "d"]);
}

#[test]
fn workspace_completer_filters_by_prefix() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login");
    save(&mgr, WorkspaceStatus::Pending, "fix-search", "Fix search");
    save(&mgr, WorkspaceStatus::Pending, "add-thing", "Add thing");

    let cands = complete_workspace_with(&mgr, OsStr::new("fix"), WorkspaceFilter::Pending);
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["fix-login", "fix-search"]);
}

#[test]
fn workspace_completer_includes_description() {
    let (_tmp, mgr) = make_mgr();
    save(&mgr, WorkspaceStatus::Pending, "fix-login", "Fix login bug");

    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Pending);
    assert_eq!(cands.len(), 1);
    let help = cands[0].get_help().unwrap().to_string();
    assert!(help.contains("Fix login bug"), "help was: {}", help);
    assert!(help.contains("pending"), "help was: {}", help);
}

#[test]
fn workspace_completer_returns_empty_when_dir_missing() {
    let tmp = TempDir::new().unwrap();
    // Do NOT call ensure_dirs
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    let cands = complete_workspace_with(&mgr, OsStr::new(""), WorkspaceFilter::Any);
    assert!(cands.is_empty());
}

fn make_repo(path: &str) -> RepoConfig {
    RepoConfig {
        path: path.into(),
        default_target_branch: None,
        copy_files: Vec::new(),
        hooks: HooksConfig::default(),
        lazygit: None,
        zellij: None,
    }
}

#[test]
fn repo_completer_lists_all_with_path_help() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/work/fe"))
        .unwrap();
    mgr.save_repo_config("backend", &make_repo("/work/be"))
        .unwrap();

    let cands = complete_repo_with(&mgr, OsStr::new(""));
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["backend", "frontend"]);

    let frontend = cands.iter().find(|c| c.get_value() == "frontend").unwrap();
    assert!(frontend
        .get_help()
        .unwrap()
        .to_string()
        .contains("/work/fe"));
}

#[test]
fn repo_completer_filters_by_prefix() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/work/fe"))
        .unwrap();
    mgr.save_repo_config("backend", &make_repo("/work/be"))
        .unwrap();
    mgr.save_repo_config("docs", &make_repo("/work/docs"))
        .unwrap();

    let cands = complete_repo_with(&mgr, OsStr::new("fr"));
    assert_eq!(names(&cands), vec!["frontend"]);
}

#[test]
fn template_completer_lists_all_with_repos_help() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_template(
        "web",
        &TemplateConfig {
            repos: vec!["frontend".into(), "backend".into()],
            zellij: ZellijConfig::default(),
        },
    )
    .unwrap();

    let cands = complete_template_with(&mgr, OsStr::new(""));
    assert_eq!(names(&cands), vec!["web"]);
    let help = cands[0].get_help().unwrap().to_string();
    assert!(
        help.contains("frontend") && help.contains("backend"),
        "help: {}",
        help
    );
}

#[test]
fn template_completer_filters_by_prefix() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_template(
        "web",
        &TemplateConfig {
            repos: vec!["a".into()],
            zellij: ZellijConfig::default(),
        },
    )
    .unwrap();
    mgr.save_template(
        "mobile",
        &TemplateConfig {
            repos: vec!["b".into()],
            zellij: ZellijConfig::default(),
        },
    )
    .unwrap();

    let cands = complete_template_with(&mgr, OsStr::new("m"));
    assert_eq!(names(&cands), vec!["mobile"]);
}

#[test]
fn repo_completer_returns_empty_when_dir_missing() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    assert!(complete_repo_with(&mgr, OsStr::new("")).is_empty());
}

#[test]
fn template_completer_returns_empty_when_dir_missing() {
    let tmp = TempDir::new().unwrap();
    let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
    assert!(complete_template_with(&mgr, OsStr::new("")).is_empty());
}

#[test]
fn repos_list_completer_handles_first_segment() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/fe")).unwrap();
    mgr.save_repo_config("backend", &make_repo("/be")).unwrap();

    let cands = complete_repos_list_with(&mgr, OsStr::new(""));
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["backend", "frontend"]);
}

#[test]
fn repos_list_completer_handles_continuation() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/fe")).unwrap();
    mgr.save_repo_config("backend", &make_repo("/be")).unwrap();

    let cands = complete_repos_list_with(&mgr, OsStr::new("frontend,"));
    let mut got = names(&cands);
    got.sort();
    assert_eq!(got, vec!["frontend,backend", "frontend,frontend"]);
}

#[test]
fn repos_list_completer_filters_partial_continuation() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/fe")).unwrap();
    mgr.save_repo_config("backend", &make_repo("/be")).unwrap();

    let cands = complete_repos_list_with(&mgr, OsStr::new("frontend,b"));
    assert_eq!(names(&cands), vec!["frontend,backend"]);
}

#[test]
fn repos_list_completer_skips_branch_segment() {
    let (_tmp, mgr) = make_mgr();
    mgr.save_repo_config("frontend", &make_repo("/fe")).unwrap();

    // current ends with `:`, indicating user is typing branch name; we don't suggest branches.
    let cands = complete_repos_list_with(&mgr, OsStr::new("frontend:"));
    assert!(cands.is_empty());
}

use clap_complete::Shell;
use zootree::cli::completions::write_registration;

fn dynamic_script(shell: Shell) -> String {
    let mut buf = Vec::new();
    write_registration(shell, "zootree", &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn dynamic_zsh_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::Zsh);
    assert!(
        s.contains("#compdef zootree"),
        "zsh script missing compdef: {s}"
    );
    assert!(
        s.contains("_clap_dynamic_completer"),
        "zsh script missing dispatcher: {s}"
    );
    assert!(
        s.contains("COMPLETE"),
        "zsh script missing COMPLETE env var: {s}"
    );
    assert!(
        !s.contains("completions:Generate"),
        "zsh script looks like AOT output (contains subcommand list): {s}"
    );
}

#[test]
fn dynamic_bash_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::Bash);
    assert!(
        s.contains("_clap_complete_"),
        "bash script missing dispatcher: {s}"
    );
    assert!(
        s.contains("COMPLETE"),
        "bash script missing COMPLETE env var: {s}"
    );
    assert!(s.contains("zootree"), "bash script missing bin name: {s}");
}

#[test]
fn dynamic_fish_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::Fish);
    assert!(
        s.contains("COMPLETE=fish"),
        "fish script missing dynamic env invocation: {s}"
    );
    assert!(
        s.contains("--command zootree"),
        "fish script missing bin name: {s}"
    );
}

#[test]
fn dynamic_powershell_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::PowerShell);
    assert!(
        s.contains("Register-ArgumentCompleter"),
        "powershell script missing registration: {s}"
    );
    assert!(
        s.contains("COMPLETE"),
        "powershell script missing COMPLETE env var: {s}"
    );
}

#[test]
fn dynamic_elvish_registration_dispatches_to_complete_env() {
    let s = dynamic_script(Shell::Elvish);
    assert!(
        s.contains("edit:completion:arg-completer"),
        "elvish script missing arg-completer binding: {s}"
    );
    assert!(
        s.contains("COMPLETE"),
        "elvish script missing COMPLETE env var: {s}"
    );
}
