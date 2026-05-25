use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::config::global::HookValue;
use zootree::core::hook::{HookContext, HookEngine};
use zootree::runner::MockRunner;

fn success_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

#[test]
fn test_simple_hook_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "calm-river".into(),
        repo: Some("frontend".into()),
        branch: "zootree/calm-river".into(),
        target_branch: Some("develop".into()),
        worktree_path: Some("/home/user/ws/calm-river/frontend".into()),
        workspace_dir: "/home/user/ws/calm-river".into(),
    };

    let hook = HookValue::Simple("npm install".into());
    engine.execute(&hook, &ctx).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].args, vec!["-c", "npm install"]);
    assert_eq!(calls[0].env.get("ZOOTREE_WORKSPACE").unwrap(), "calm-river");
    assert_eq!(calls[0].env.get("ZOOTREE_REPO").unwrap(), "frontend");
    assert_eq!(
        calls[0].env.get("ZOOTREE_BRANCH").unwrap(),
        "zootree/calm-river"
    );
    assert_eq!(
        calls[0].env.get("ZOOTREE_TARGET_BRANCH").unwrap(),
        "develop"
    );
    assert_eq!(
        calls[0].env.get("ZOOTREE_WORKTREE_PATH").unwrap(),
        "/home/user/ws/calm-river/frontend"
    );
    assert_eq!(
        calls[0].env.get("ZOOTREE_WORKSPACE_DIR").unwrap(),
        "/home/user/ws/calm-river"
    );
}

#[test]
fn test_file_hook_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "calm-river".into(),
        repo: None,
        branch: "zootree/calm-river".into(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: "/home/user/ws/calm-river".into(),
    };

    let hook = HookValue::File {
        file: "/home/user/.config/zootree/hooks/cleanup.sh".into(),
    };
    engine.execute(&hook, &ctx).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].program, "sh");
    assert_eq!(
        calls[0].args,
        vec!["/home/user/.config/zootree/hooks/cleanup.sh"]
    );
}

#[test]
fn test_file_hook_expands_tilde_path() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "calm-river".into(),
        repo: None,
        branch: "zootree/calm-river".into(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: "/home/user/ws/calm-river".into(),
    };

    let hook = HookValue::File {
        file: "~/.config/zootree/hooks/cleanup.sh".into(),
    };
    engine.execute(&hook, &ctx).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].program, "sh");
    assert_ne!(calls[0].args[0], "~/.config/zootree/hooks/cleanup.sh");
    assert!(
        calls[0].args[0].ends_with("/.config/zootree/hooks/cleanup.sh"),
        "expanded path should keep hook suffix, got: {:?}",
        calls[0].args
    );
}

#[test]
fn test_inline_hook_command() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "calm-river".into(),
        repo: None,
        branch: "zootree/calm-river".into(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: "/home/user/ws/calm-river".into(),
    };

    let script = "cd $ZOOTREE_WORKTREE_PATH\nnpm install\nnpm run db:migrate";
    let hook = HookValue::Inline {
        inline: script.into(),
    };
    engine.execute(&hook, &ctx).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].args, vec!["-c", script]);
}

#[test]
fn test_hook_failure_returns_error() {
    let runner = MockRunner::new();
    runner.push_response(Output {
        status: ExitStatus::from_raw(256), // exit code 1
        stdout: Vec::new(),
        stderr: b"command not found".to_vec(),
    });
    let engine = HookEngine::new(&runner);

    let ctx = HookContext {
        workspace: "test".into(),
        repo: None,
        branch: "zootree/test".into(),
        target_branch: None,
        worktree_path: None,
        workspace_dir: "/tmp/test".into(),
    };

    let hook = HookValue::Simple("bad-command".into());
    let result = engine.execute(&hook, &ctx);
    assert!(result.is_err());
}
