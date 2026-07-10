use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::core::multiplexer::{
    zellij::{plan_launch, LaunchPlan, ZellijMultiplexer},
    MultiplexerIdentity, MultiplexerLaunch, TerminalMultiplexer,
};
use zootree::runner::{CommandRunner, CommandSpec, MockRunner};

fn success_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

fn launch() -> MultiplexerLaunch {
    MultiplexerLaunch {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        description: "Fix cmux sidebar copy".into(),
        workspace_dir: "/tmp/fair-fox".into(),
        layout_name: "default".into(),
        rendered_layout: "layout {}".into(),
        layout_file: "/tmp/layout.kdl".into(),
    }
}

fn identity() -> MultiplexerIdentity {
    MultiplexerIdentity {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        cmux_workspace: None,
    }
}

#[test]
fn test_kill_session_calls_delete_force_only() {
    let runner = MockRunner::new();
    runner.push_response(success_output());
    let zellij = ZellijMultiplexer::new(&runner, false);

    zellij.close(&identity()).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1, "expected exactly one zellij call");
    assert_eq!(calls[0].program, "zellij");
    assert_eq!(
        calls[0].args,
        vec!["delete-session", "--force", "zootree-fair-fox"]
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
    runner.push_response(stdout_output(b"other-session\n"));
    runner.push_response(success_output());
    let zellij = ZellijMultiplexer::new(&runner, true);

    zellij.launch(&launch()).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    let c = &calls[1];
    assert_eq!(c.program, "zellij");
    assert_eq!(
        c.args,
        vec![
            "-l",
            "/tmp/layout.kdl",
            "attach",
            "--create-background",
            "zootree-fair-fox"
        ]
    );
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ"));
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ_SESSION_NAME"));
    assert!(c.env_remove.iter().any(|k| k == "ZELLIJ_PANE_ID"));
}

#[test]
fn start_session_background_propagates_failure_with_stderr() {
    let runner = MockRunner::new();
    runner.push_response(stdout_output(b"other-session\n"));
    runner.push_response(failure_output("zellij: layout parse error"));
    let zellij = ZellijMultiplexer::new(&runner, true);

    let err = zellij.launch(&launch()).unwrap_err();
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
fn launch_inside_zellij_no_session_creates_background() {
    let runner = MockRunner::new();
    // session_exists -> list-sessions returns lines without our session
    runner.push_response(stdout_output(b"other-session\n"));
    // start_session_background succeeds
    runner.push_response(success_output());

    let zellij = ZellijMultiplexer::new(&runner, true);
    zellij.launch(&launch()).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["list-sessions"]);
    assert_eq!(calls[1].program, "zellij");
    assert!(calls[1].args.contains(&"--create-background".to_string()));
    assert!(calls[1].args.contains(&"zootree-fair-fox".to_string()));
}

#[test]
fn launch_inside_zellij_session_exists_invokes_only_list_sessions() {
    let runner = MockRunner::new();
    // list-sessions includes our session
    runner.push_response(stdout_output(b"zootree-fair-fox\nother-session\n"));

    let zellij = ZellijMultiplexer::new(&runner, true);
    zellij.launch(&launch()).unwrap();

    let calls = runner.take_calls();
    assert_eq!(
        calls.len(),
        1,
        "should only call list-sessions, no follow-up"
    );
    assert_eq!(calls[0].args, vec!["list-sessions"]);
}

#[test]
fn launch_propagates_list_sessions_failure() {
    let runner = MockRunner::new();
    runner.push_response(failure_output("zellij socket unavailable"));

    let zellij = ZellijMultiplexer::new(&runner, true);
    let err = zellij.launch(&launch()).unwrap_err();
    let msg = format!("{:#}", err);

    assert!(
        msg.contains("zellij list-sessions failed") && msg.contains("zellij socket unavailable"),
        "unexpected error: {msg}"
    );
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1, "failed list-sessions must not create");
    assert_eq!(calls[0].args, vec!["list-sessions"]);
}

#[test]
fn launch_treats_ansi_styled_session_name_as_existing() {
    let runner = MockRunner::new();
    runner.push_response(stdout_output(
        b"\x1b[32mzootree-fair-fox\x1b[0m\nother-session\n",
    ));

    let zellij = ZellijMultiplexer::new(&runner, true);
    zellij.launch(&launch()).unwrap();

    let calls = runner.take_calls();
    assert_eq!(
        calls.len(),
        1,
        "styled existing session should not create a new session"
    );
    assert_eq!(calls[0].args, vec!["list-sessions"]);
}

#[test]
fn launch_does_not_treat_session_name_prefix_as_existing() {
    let runner = MockRunner::new();
    runner.push_response(stdout_output(b"zootree-fair-fox-old\nother-session\n"));
    runner.push_response(success_output());

    let zellij = ZellijMultiplexer::new(&runner, true);
    zellij.launch(&launch()).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["list-sessions"]);
    assert!(
        calls[1].args.contains(&"--create-background".to_string()),
        "expected missing exact session to create in background, got {:?}",
        calls[1].args
    );
}

#[test]
fn launch_outside_zellij_no_session_calls_start_session() {
    let runner = MockRunner::new();
    runner.push_response(stdout_output(b"")); // list-sessions empty
    runner.push_response(success_output()); // start_session (interactive) ok

    let zellij = ZellijMultiplexer::new(&runner, false);
    zellij.launch(&launch()).unwrap();

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
