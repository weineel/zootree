use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::core::git::GitOps;
use zootree::runner::MockRunner;

fn success_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

#[test]
fn test_worktree_add_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.worktree_add(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "/home/user/zootree-workspaces/calm-river/frontend",
        "develop",
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "git");
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "worktree",
            "add",
            "-b",
            "zootree/calm-river",
            "/home/user/zootree-workspaces/calm-river/frontend",
            "develop",
        ]
    );
}

#[test]
fn test_worktree_remove_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.worktree_remove(
        "/home/user/projects/frontend",
        "/home/user/zootree-workspaces/calm-river/frontend",
        false,
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "git");
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "worktree",
            "remove",
            "/home/user/zootree-workspaces/calm-river/frontend",
        ]
    );
}

#[test]
fn test_worktree_remove_force() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.worktree_remove(
        "/home/user/projects/frontend",
        "/home/user/zootree-workspaces/calm-river/frontend",
        true,
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "worktree",
            "remove",
            "--force",
            "/home/user/zootree-workspaces/calm-river/frontend",
        ]
    );
}

#[test]
fn test_merge_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // checkout
    runner.push_response(success_output()); // merge
    let git = GitOps::new(&runner);

    git.merge(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "develop",
        Some("merge"),
        "squash merge zootree/calm-river",
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].args,
        vec!["-C", "/home/user/projects/frontend", "checkout", "develop"]
    );
    assert_eq!(
        calls[1].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "merge",
            "zootree/calm-river"
        ]
    );
}

#[test]
fn test_merge_rebase_command_order() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // rebase target branch
    runner.push_response(success_output()); // checkout target branch
    runner.push_response(success_output()); // fast-forward target
    let git = GitOps::new(&runner);

    git.merge_with_worktree(
        "/home/user/projects/frontend",
        Some("/home/user/zootree-workspaces/calm-river/frontend"),
        "zootree/calm-river",
        "develop",
        Some("rebase"),
        "unused for rebase",
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/home/user/zootree-workspaces/calm-river/frontend",
            "rebase",
            "develop"
        ]
    );
    assert_eq!(
        calls[1].args,
        vec!["-C", "/home/user/projects/frontend", "checkout", "develop"]
    );
    assert_eq!(
        calls[2].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "merge",
            "--ff-only",
            "zootree/calm-river"
        ]
    );
}

#[test]
fn test_merge_rebase_requires_worktree_path() {
    let runner = MockRunner::new();
    let git = GitOps::new(&runner);

    let err = git
        .merge_with_worktree(
            "/home/user/projects/frontend",
            None,
            "zootree/calm-river",
            "develop",
            Some("rebase"),
            "unused for rebase",
        )
        .unwrap_err();

    assert!(err
        .to_string()
        .contains("rebase strategy requires branch worktree path"));
    assert!(runner.take_calls().is_empty());
}

#[test]
fn test_merge_squash() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // checkout
    runner.push_response(success_output()); // merge --squash
    runner.push_response(Output {
        // diff --staged --quiet: exit 1 = has staged changes
        status: ExitStatus::from_raw(256),
        stdout: Vec::new(),
        stderr: Vec::new(),
    });
    runner.push_response(success_output()); // commit
    let git = GitOps::new(&runner);

    git.merge(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "develop",
        Some("squash"),
        "fix: resolve login issue",
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(
        calls[1].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "merge",
            "--squash",
            "zootree/calm-river"
        ]
    );
    assert_eq!(
        calls[2].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "diff",
            "--staged",
            "--quiet"
        ]
    );
    assert_eq!(
        calls[3].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "commit",
            "-m",
            "fix: resolve login issue"
        ]
    );
}

#[test]
fn test_merge_squash_nothing_to_merge() {
    let runner = MockRunner::new();
    runner.push_response(success_output()); // checkout
    runner.push_response(success_output()); // merge --squash
    runner.push_response(success_output()); // diff --staged --quiet: exit 0 = nothing staged
    let git = GitOps::new(&runner);

    git.merge(
        "/home/user/projects/frontend",
        "zootree/calm-river",
        "develop",
        None, // default = squash
        "fix: resolve login issue",
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[2].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "diff",
            "--staged",
            "--quiet"
        ]
    );
}

#[test]
fn test_push_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.push("/home/user/projects/frontend", "develop").unwrap();

    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "push",
            "origin",
            "develop"
        ]
    );
}

#[test]
fn test_delete_local_branch() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let git = GitOps::new(&runner);

    git.delete_local_branch("/home/user/projects/frontend", "zootree/calm-river", false)
        .unwrap();

    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/home/user/projects/frontend",
            "branch",
            "-d",
            "zootree/calm-river"
        ]
    );
}

#[test]
fn test_has_uncommitted_changes() {
    let runner = MockRunner::new();
    runner.push_response(Output {
        status: ExitStatus::from_raw(0),
        stdout: b" M src/main.rs\n".to_vec(),
        stderr: Vec::new(),
    });
    let git = GitOps::new(&runner);

    let result = git
        .has_uncommitted_changes("/home/user/worktree/frontend")
        .unwrap();
    assert!(result);

    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec![
            "-C",
            "/home/user/worktree/frontend",
            "status",
            "--porcelain"
        ]
    );
}
