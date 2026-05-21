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
