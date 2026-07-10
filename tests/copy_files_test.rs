use std::fs;
use std::os::unix::fs::PermissionsExt;
use zootree::core::copy_files::{copy_files_to_worktree, merge_copy_files};

#[test]
fn test_merge_copy_files_combines() {
    let global = vec![".env".to_string()];
    let repo = vec![
        ".env.local".to_string(),
        ".vscode/settings.json".to_string(),
    ];
    let merged = merge_copy_files(&global, &repo);
    assert_eq!(merged, vec![".env", ".env.local", ".vscode/settings.json"]);
}

#[test]
fn test_merge_copy_files_dedup() {
    let global = vec![".env".to_string()];
    let repo = vec![".env".to_string(), ".env.local".to_string()];
    let merged = merge_copy_files(&global, &repo);
    assert_eq!(merged, vec![".env", ".env.local"]);
}

#[test]
fn test_merge_copy_files_empty() {
    let global: Vec<String> = vec![];
    let repo: Vec<String> = vec![];
    let merged = merge_copy_files(&global, &repo);
    assert!(merged.is_empty());
}

#[test]
fn copy_files_reports_glob_iteration_errors() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let worktree = temp.path().join("worktree");
    let private = repo.join("private");
    fs::create_dir_all(&private).unwrap();
    fs::create_dir_all(&worktree).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o000)).unwrap();

    let result = copy_files_to_worktree(&repo, &worktree, &["private/*".into()]);

    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    let err = result.unwrap_err();
    let msg = format!("{:#}", err);
    assert!(
        msg.contains("copy_files pattern 'private/*' failed while reading matches"),
        "unexpected error: {msg}"
    );
}

#[test]
fn copy_files_skips_directories_matched_by_glob() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let worktree = temp.path().join("worktree");
    fs::create_dir_all(repo.join("config/nested")).unwrap();
    fs::create_dir_all(&worktree).unwrap();
    fs::write(repo.join("config/app.toml"), "debug = true").unwrap();
    fs::write(repo.join("config/nested/secret.toml"), "token = 'x'").unwrap();

    copy_files_to_worktree(&repo, &worktree, &["config/*".into()]).unwrap();

    assert_eq!(
        fs::read_to_string(worktree.join("config/app.toml")).unwrap(),
        "debug = true"
    );
    assert!(
        !worktree.join("config/nested").exists(),
        "directories matched by copy_files should be skipped, not copied recursively"
    );
}

#[test]
fn copy_files_skips_plain_directory_patterns() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let worktree = temp.path().join("worktree");
    fs::create_dir_all(repo.join("config")).unwrap();
    fs::create_dir_all(&worktree).unwrap();
    fs::write(repo.join("config/app.toml"), "debug = true").unwrap();

    copy_files_to_worktree(&repo, &worktree, &["config".into()]).unwrap();

    assert!(
        !worktree.join("config").exists(),
        "plain directory copy_files entries should be skipped explicitly"
    );
}
