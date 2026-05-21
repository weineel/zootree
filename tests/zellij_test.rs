use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::core::zellij::ZellijOps;
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
