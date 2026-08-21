use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::core::editor::open_file_with;
use zootree::runner::MockRunner;

fn successful_output() -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

fn failed_output() -> Output {
    Output {
        status: ExitStatus::from_raw(7 << 8),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

#[test]
fn editor_prefers_visual_and_supports_command_arguments() {
    let runner = MockRunner::new();
    runner.push_response(successful_output());

    open_file_with(
        "/tmp/config.toml".as_ref(),
        &runner,
        Some("code --wait"),
        Some("vim"),
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "code");
    assert_eq!(calls[0].args, vec!["--wait", "/tmp/config.toml"]);
}

#[test]
fn editor_reports_a_nonzero_exit_status() {
    let runner = MockRunner::new();
    runner.push_response(failed_output());

    let error =
        open_file_with("/tmp/config.toml".as_ref(), &runner, Some("code"), None).unwrap_err();

    assert!(error.to_string().contains("code"), "error: {error:#}");
    assert!(error.to_string().contains('7'), "error: {error:#}");
}

#[test]
fn editor_ignores_an_empty_visual_value() {
    let runner = MockRunner::new();
    runner.push_response(successful_output());

    open_file_with(
        "/tmp/config.toml".as_ref(),
        &runner,
        Some("   "),
        Some("vim -f"),
    )
    .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].program, "vim");
    assert_eq!(calls[0].args, vec!["-f", "/tmp/config.toml"]);
}
