use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::core::zellij::{plan_launch, LaunchPlan, ZellijOps};
use zootree::runner::{CommandRunner, CommandSpec, MockRunner};

fn success_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

#[test]
fn test_kill_session_calls_delete_force_only() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let zellij = ZellijOps::new(&runner);

    zellij.kill_session("zootree-test-ws").unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1, "expected exactly one zellij call");
    assert_eq!(calls[0].program, "zellij");
    assert_eq!(
        calls[0].args,
        vec!["delete-session", "--force", "zootree-test-ws"]
    );
}

#[test]
fn mock_runner_preserves_env_remove() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let spec = CommandSpec {
        program: "echo".into(),
        args: vec!["hi".into()],
        cwd: None,
        env: HashMap::new(),
        env_remove: vec!["FOO".into(), "BAR".into()],
    };
    runner.run(&spec).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].env_remove,
        vec!["FOO".to_string(), "BAR".to_string()]
    );
}

#[test]
fn plan_launch_outside_no_session_yields_foreground_create() {
    assert_eq!(plan_launch(false, false), LaunchPlan::ForegroundCreate);
}

#[test]
fn plan_launch_outside_session_exists_yields_foreground_attach() {
    assert_eq!(plan_launch(false, true), LaunchPlan::ForegroundAttach);
}

#[test]
fn plan_launch_inside_no_session_yields_background_create() {
    assert_eq!(plan_launch(true, false), LaunchPlan::BackgroundCreate);
}

#[test]
fn plan_launch_inside_session_exists_yields_already_running_hint() {
    assert_eq!(plan_launch(true, true), LaunchPlan::AlreadyRunningHint);
}

use std::path::Path;

fn failure_output(stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(1 << 8), // wait-status: exit code 1
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn start_session_background_invokes_zellij_with_correct_args_and_env_remove() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let zellij = ZellijOps::new(&runner);

    zellij
        .start_session_background("ws-foo", Path::new("/tmp/layout.kdl"))
        .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    let c = &calls[0];
    assert_eq!(c.program, "zellij");
    assert_eq!(
        c.args,
        vec![
            "-l",
            "/tmp/layout.kdl",
            "attach",
            "--create-background",
            "ws-foo"
        ]
    );
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ"));
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ_SESSION_NAME"));
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ_PANE_ID"));
}

#[test]
fn start_session_background_propagates_failure_with_stderr() {
    let runner = MockRunner::new();
    runner.push_response(failure_output("zellij: layout parse error"));
    let zellij = ZellijOps::new(&runner);

    let err = zellij
        .start_session_background("ws-foo", Path::new("/tmp/layout.kdl"))
        .unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("layout parse error"),
        "expected stderr propagated, got: {}",
        msg
    );
}

fn stdout_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

#[test]
fn dispatch_launch_inside_zellij_no_session_creates_background() {
    use zootree::cli::workspace::dispatch_launch;
    let runner = MockRunner::new();
    // session_exists -> list-sessions returns lines without our session
    runner.push_response(stdout_output(b"other-session\n"));
    // start_session_background succeeds
    runner.push_response(success_output());

    let zellij = ZellijOps::new(&runner);
    dispatch_launch(
        &zellij,
        "fair-fox",         // workspace_name (used in printed hint)
        "zootree-fair-fox", // session_name
        Path::new("/tmp/layout.kdl"),
        true, // in_zellij
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["list-sessions"]);
    assert_eq!(calls[1].program, "zellij");
    assert!(calls[1].args.contains(&"--create-background".to_string()));
    assert!(calls[1].args.contains(&"zootree-fair-fox".to_string()));
}

#[test]
fn dispatch_launch_inside_zellij_session_exists_invokes_only_list_sessions() {
    use zootree::cli::workspace::dispatch_launch;
    let runner = MockRunner::new();
    // list-sessions includes our session
    runner.push_response(stdout_output(b"zootree-fair-fox\nother-session\n"));

    let zellij = ZellijOps::new(&runner);
    dispatch_launch(
        &zellij,
        "fair-fox",
        "zootree-fair-fox",
        Path::new("/tmp/layout.kdl"),
        true,
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(
        calls.len(),
        1,
        "should only call list-sessions, no follow-up"
    );
    assert_eq!(calls[0].args, vec!["list-sessions"]);
}

#[test]
fn dispatch_launch_outside_zellij_no_session_calls_start_session() {
    use zootree::cli::workspace::dispatch_launch;
    let runner = MockRunner::new();
    runner.push_response(stdout_output(b"")); // list-sessions empty
    runner.push_response(success_output()); // start_session (interactive) ok

    let zellij = ZellijOps::new(&runner);
    dispatch_launch(
        &zellij,
        "fair-fox",
        "zootree-fair-fox",
        Path::new("/tmp/layout.kdl"),
        false,
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["list-sessions"]);
    assert!(
        calls[1]
            .args
            .iter()
            .any(|a| a == "--new-session-with-layout"),
        "expected start_session via --new-session-with-layout, got {:?}",
        calls[1].args
    );
}
