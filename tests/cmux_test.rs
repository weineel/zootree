use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::core::multiplexer::{
    cmux::CmuxMultiplexer, LaunchOutcome, MultiplexerIdentity, MultiplexerLaunch,
    TerminalMultiplexer,
};
use zootree::runner::MockRunner;

fn success_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

fn failure_output(stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(1),
        stdout: Vec::new(),
        stderr: stderr.to_vec(),
    }
}

fn launch() -> MultiplexerLaunch {
    MultiplexerLaunch {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        description: "Fix cmux sidebar copy".into(),
        workspace_dir: "/tmp/fair-fox".into(),
        layout_name: "default".into(),
        rendered_layout: r#"{"pane":{"surfaces":[{"type":"terminal"}]}}"#.into(),
        layout_file: "/tmp/default.cmux.json".into(),
    }
}

fn identity(cmux_workspace: Option<&str>) -> MultiplexerIdentity {
    MultiplexerIdentity {
        workspace_name: "fair-fox".into(),
        display_name: "zootree-fair-fox".into(),
        cmux_workspace: cmux_workspace.map(str::to_string),
    }
}

#[test]
fn launch_invokes_cmux_new_workspace() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:7\n"));
    let cmux = CmuxMultiplexer::new(&runner);

    assert_eq!(cmux.launch(&launch()).unwrap(), LaunchOutcome::Launched);

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "cmux");
    assert_eq!(
        calls[0].args,
        vec![
            "workspace",
            "create",
            "--name",
            "zootree-fair-fox",
            "--description",
            "Fix cmux sidebar copy",
            "--cwd",
            "/tmp/fair-fox",
            "--layout",
            r#"{"pane":{"surfaces":[{"type":"terminal"}]}}"#,
            "--focus",
            "true"
        ]
    );
    assert_eq!(calls[0].env, HashMap::new());
}

#[test]
fn close_uses_persisted_workspace_ref() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.close(&identity(Some("workspace:7"))).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace", "close", "workspace:7"]);
}

#[test]
fn open_selects_persisted_workspace_ref() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    assert_eq!(
        cmux.open(&launch(), &identity(Some("workspace:7")))
            .unwrap(),
        LaunchOutcome::Attached
    );

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["workspace", "select", "workspace:7"]);
}

#[test]
fn launch_or_open_recreates_when_persisted_workspace_ref_cannot_be_selected() {
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"workspace not found"));
    runner.push_response(success_output(b"workspace:8\n"));
    let cmux = CmuxMultiplexer::new(&runner);

    assert_eq!(
        cmux.launch_or_open_and_capture_workspace(&launch(), &identity(Some("workspace:7")))
            .unwrap()
            .as_deref(),
        Some("workspace:8")
    );

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["workspace", "select", "workspace:7"]);
    assert_eq!(
        calls[1].args,
        vec![
            "workspace",
            "create",
            "--name",
            "zootree-fair-fox",
            "--description",
            "Fix cmux sidebar copy",
            "--cwd",
            "/tmp/fair-fox",
            "--layout",
            r#"{"pane":{"surfaces":[{"type":"terminal"}]}}"#,
            "--focus",
            "true"
        ]
    );
}

#[test]
fn parse_workspace_ref_ignores_non_workspace_lines() {
    let output = success_output(b"created cmux workspace\nworkspace:9 zootree-fair-fox\n");

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_workspace_ref(&output).as_deref(),
        Some("workspace:9")
    );
}

#[test]
fn parse_workspace_ref_ignores_non_numeric_workspace_tokens() {
    let output = success_output(b"workspace:bogus\nworkspace:9,\nworkspace:10 zootree-fair-fox\n");

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_workspace_ref(&output).as_deref(),
        Some("workspace:10")
    );
}

#[test]
fn close_without_id_skips_when_lookup_has_no_match() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:1 other\n"));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.close(&identity(None)).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace", "list"]);
}

#[test]
fn close_without_id_closes_unique_name_match() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:4 zootree-fair-fox\n"));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.close(&identity(None)).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, vec!["workspace", "list"]);
    assert_eq!(calls[1].args, vec!["workspace", "close", "workspace:4"]);
}

#[test]
fn close_without_id_skips_suffix_only_name_match() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:8 backup-zootree-fair-fox\n"));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.close(&identity(None)).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace", "list"]);
}

#[test]
fn close_without_id_skips_duplicate_exact_name_matches() {
    let runner = MockRunner::new();
    runner.push_response(success_output(
        b"workspace:4 zootree-fair-fox\nworkspace:8 zootree-fair-fox\n",
    ));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.close(&identity(None)).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace", "list"]);
}
