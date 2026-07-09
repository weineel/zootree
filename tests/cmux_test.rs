use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use zootree::core::multiplexer::{
    cmux::{CmuxGroupFocusOutcome, CmuxMultiplexer},
    CmuxGroupLaunch, CmuxRepoWorkspaceLaunch, LaunchOutcome, MultiplexerIdentity,
    MultiplexerLaunch, TerminalMultiplexer,
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

fn group_launch() -> CmuxGroupLaunch {
    CmuxGroupLaunch {
        workspace_name: "fair-fox".into(),
        group_name: "Fix cmux sidebar copy".into(),
        anchor_name: "zootree-fair-fox".into(),
        anchor_description: "Fix cmux sidebar copy".into(),
        anchor_cwd: "/tmp/fair-fox".into(),
        anchor_layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"info"}]}}"#.into(),
        repo_workspaces: vec![
            CmuxRepoWorkspaceLaunch {
                repo_name: "api".into(),
                workspace_name: "zootree-fair-fox-api".into(),
                description: "api".into(),
                cwd: "/tmp/fair-fox/api".into(),
                layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"api"}]}}"#.into(),
            },
            CmuxRepoWorkspaceLaunch {
                repo_name: "web".into(),
                workspace_name: "zootree-fair-fox-web".into(),
                description: "web".into(),
                cwd: "/tmp/fair-fox/web".into(),
                layout: r#"{"pane":{"surfaces":[{"type":"terminal","name":"web"}]}}"#.into(),
            },
        ],
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

#[test]
fn launch_group_creates_anchor_group_and_repo_workspaces() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:4\n"));
    runner.push_response(success_output(b"workspace_group:2\n"));
    runner.push_response(success_output(
        br#"{"groups":[{"name":"Fix cmux sidebar copy","ref":"workspace_group:2","anchor_workspace_ref":"workspace:99"}]}"#,
    ));
    runner.push_response(success_output(b"workspace:7\n"));
    runner.push_response(success_output(b""));
    runner.push_response(success_output(b""));
    runner.push_response(success_output(b"workspace:5\n"));
    let cmux = CmuxMultiplexer::new(&runner);

    let state = cmux
        .launch_group_and_capture_state(&group_launch())
        .unwrap();

    assert_eq!(state.group, "workspace_group:2");
    assert_eq!(state.repo_workspaces.len(), 2);
    assert_eq!(state.repo_workspaces[0].repo, "api");
    assert_eq!(state.repo_workspaces[0].workspace, "workspace:4");
    assert_eq!(state.repo_workspaces[1].repo, "web");
    assert_eq!(state.repo_workspaces[1].workspace, "workspace:5");

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 7);
    assert_eq!(
        calls[0].args,
        vec![
            "workspace",
            "create",
            "--name",
            "zootree-fair-fox-api",
            "--description",
            "api",
            "--cwd",
            "/tmp/fair-fox/api",
            "--layout",
            r#"{"pane":{"surfaces":[{"type":"terminal","name":"api"}]}}"#,
            "--focus",
            "true"
        ]
    );
    assert_eq!(
        calls[1].args,
        vec![
            "workspace-group",
            "create",
            "--name",
            "Fix cmux sidebar copy",
            "--from",
            "workspace:4"
        ]
    );
    assert_eq!(calls[2].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(
        calls[3].args,
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
            r#"{"pane":{"surfaces":[{"type":"terminal","name":"info"}]}}"#,
            "--focus",
            "true",
            "--group",
            "workspace_group:2",
            "--group-placement",
            "top"
        ]
    );
    assert_eq!(
        calls[4].args,
        vec![
            "workspace-group",
            "set-anchor",
            "--group",
            "workspace_group:2",
            "--workspace",
            "workspace:7"
        ]
    );
    assert_eq!(calls[5].args, vec!["workspace", "close", "workspace:99"]);
    assert_eq!(
        calls[6].args,
        vec![
            "workspace",
            "create",
            "--name",
            "zootree-fair-fox-web",
            "--description",
            "web",
            "--cwd",
            "/tmp/fair-fox/web",
            "--layout",
            r#"{"pane":{"surfaces":[{"type":"terminal","name":"web"}]}}"#,
            "--focus",
            "false",
            "--group",
            "workspace_group:2",
            "--group-placement",
            "end"
        ]
    );
}

#[test]
fn launch_group_group_ref_error_includes_group_name() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:4\n"));
    runner.push_response(success_output(b"created group\n"));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    let error = cmux
        .launch_group_and_capture_state(&group_launch())
        .unwrap_err()
        .to_string();

    assert!(error.contains("Fix cmux sidebar copy"));
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[1].args,
        vec![
            "workspace-group",
            "create",
            "--name",
            "Fix cmux sidebar copy",
            "--from",
            "workspace:4"
        ]
    );
    assert_eq!(calls[2].args, vec!["workspace", "close", "workspace:4"]);
}

#[test]
fn launch_group_rolls_back_group_when_repo_workspace_create_fails() {
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"repo create failed"));
    let cmux = CmuxMultiplexer::new(&runner);

    let error = cmux
        .launch_group_and_capture_state(&group_launch())
        .unwrap_err();
    let msg = format!("{:#}", error);

    assert!(
        msg.contains("cmux workspace create failed: repo create failed"),
        "unexpected error: {msg}"
    );
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args[0], "workspace");
    assert_eq!(calls[0].args[1], "create");
}

#[test]
fn launch_group_rolls_back_group_when_second_repo_workspace_create_fails() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b"workspace:4\n"));
    runner.push_response(success_output(b"workspace_group:2\n"));
    runner.push_response(success_output(
        br#"{"groups":[{"name":"Fix cmux sidebar copy","ref":"workspace_group:2","anchor_workspace_ref":"workspace:99"}]}"#,
    ));
    runner.push_response(success_output(b"workspace:7\n"));
    runner.push_response(success_output(b""));
    runner.push_response(success_output(b""));
    runner.push_response(failure_output(b"second repo create failed"));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    let error = cmux
        .launch_group_and_capture_state(&group_launch())
        .unwrap_err();
    let msg = format!("{:#}", error);

    assert!(
        msg.contains("cmux workspace create failed: second repo create failed"),
        "unexpected error: {msg}"
    );
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 8);
    assert_eq!(
        calls[7].args,
        vec!["workspace-group", "delete", "workspace_group:2"]
    );
}

#[test]
fn parse_workspace_group_ref_finds_group_ref() {
    let output = success_output(b"created workspace_group:9\n");

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_workspace_group_ref(&output).as_deref(),
        Some("workspace_group:9")
    );
}

#[test]
fn parse_unique_group_match_finds_exact_unique_name_from_json() {
    let stdout = br#"{
  "groups": [
    { "ref": "workspace_group:2", "name": "Fix cmux sidebar copy" },
    { "ref": "workspace_group:3", "name": "Other work" }
  ],
  "window_ref": "window:1"
}"#;

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_unique_group_match(stdout, "Fix cmux sidebar copy")
            .as_deref(),
        Some("workspace_group:2")
    );
}

#[test]
fn parse_unique_group_match_rejects_duplicate_names() {
    let stdout = br#"{
  "groups": [
    { "ref": "workspace_group:2", "name": "Fix cmux sidebar copy" },
    { "ref": "workspace_group:3", "name": "Fix cmux sidebar copy" }
  ]
}"#;

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_unique_group_match(stdout, "Fix cmux sidebar copy"),
        None
    );
}

#[test]
fn parse_unique_group_match_rejects_duplicate_name_even_when_one_ref_is_invalid() {
    let stdout = br#"{
  "groups": [
    { "ref": "workspace_group:2", "name": "Fix cmux sidebar copy" },
    { "ref": "workspace_group:bogus", "name": "Fix cmux sidebar copy" }
  ]
}"#;

    assert_eq!(
        CmuxMultiplexer::<MockRunner>::parse_unique_group_match(stdout, "Fix cmux sidebar copy"),
        None
    );
}

#[test]
fn focus_group_uses_persisted_group_ref() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    let outcome = cmux
        .focus_group_or_find("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    assert_eq!(outcome, CmuxGroupFocusOutcome::FocusedExisting);
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].args,
        vec!["workspace-group", "focus", "workspace_group:2"]
    );
}

#[test]
fn focus_group_falls_back_to_unique_title_match() {
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"group not found"));
    runner.push_response(success_output(
        br#"{"groups":[{"ref":"workspace_group:7","name":"Fix cmux sidebar copy"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    let found = cmux
        .focus_group_or_find("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    assert_eq!(
        found,
        CmuxGroupFocusOutcome::FocusedFound("workspace_group:7".into())
    );
    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec!["workspace-group", "focus", "workspace_group:2"]
    );
    assert_eq!(calls[1].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(
        calls[2].args,
        vec!["workspace-group", "focus", "workspace_group:7"]
    );
}

#[test]
fn focus_group_reports_not_found_after_stale_ref_and_no_unique_title_match() {
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"group not found"));
    runner.push_response(success_output(
        br#"{"groups":[
            {"ref":"workspace_group:7","name":"Other work"}
        ]}"#,
    ));
    let cmux = CmuxMultiplexer::new(&runner);

    let outcome = cmux
        .focus_group_or_find("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    assert_eq!(outcome, CmuxGroupFocusOutcome::NotFound);
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].args,
        vec!["workspace-group", "focus", "workspace_group:2"]
    );
    assert_eq!(calls[1].args, vec!["workspace-group", "list", "--json"]);
}

#[test]
fn focus_group_reports_ambiguous_after_stale_ref_and_duplicate_title_match() {
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"group not found"));
    runner.push_response(success_output(
        br#"{"groups":[
            {"ref":"workspace_group:7","name":"Fix cmux sidebar copy"},
            {"ref":"workspace_group:8","name":"Fix cmux sidebar copy"}
        ]}"#,
    ));
    let cmux = CmuxMultiplexer::new(&runner);

    let outcome = cmux
        .focus_group_or_find("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    assert_eq!(outcome, CmuxGroupFocusOutcome::Ambiguous);
    let calls = runner.take_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].args,
        vec!["workspace-group", "focus", "workspace_group:2"]
    );
    assert_eq!(calls[1].args, vec!["workspace-group", "list", "--json"]);
}

#[test]
fn delete_group_uses_persisted_group_ref() {
    let runner = MockRunner::new();
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.delete_group("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].args,
        vec!["workspace-group", "delete", "workspace_group:2"]
    );
}

#[test]
fn delete_group_with_stale_ref_falls_back_to_unique_title_match() {
    let runner = MockRunner::new();
    runner.push_response(failure_output(b"group not found"));
    runner.push_response(success_output(
        br#"{"groups":[{"ref":"workspace_group:7","name":"Fix cmux sidebar copy"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.delete_group("Fix cmux sidebar copy", Some("workspace_group:2"))
        .unwrap();

    let calls = runner.take_calls();
    assert_eq!(
        calls[0].args,
        vec!["workspace-group", "delete", "workspace_group:2"]
    );
    assert_eq!(calls[1].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(
        calls[2].args,
        vec!["workspace-group", "delete", "workspace_group:7"]
    );
}

#[test]
fn delete_group_without_ref_uses_unique_title_match() {
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[{"ref":"workspace_group:7","name":"Fix cmux sidebar copy"}]}"#,
    ));
    runner.push_response(success_output(b""));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.delete_group("Fix cmux sidebar copy", None).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls[0].args, vec!["workspace-group", "list", "--json"]);
    assert_eq!(
        calls[1].args,
        vec!["workspace-group", "delete", "workspace_group:7"]
    );
}

#[test]
fn delete_group_without_unique_match_skips_delete() {
    let runner = MockRunner::new();
    runner.push_response(success_output(
        br#"{"groups":[
            {"ref":"workspace_group:7","name":"Fix cmux sidebar copy"},
            {"ref":"workspace_group:8","name":"Fix cmux sidebar copy"}
        ]}"#,
    ));
    let cmux = CmuxMultiplexer::new(&runner);

    cmux.delete_group("Fix cmux sidebar copy", None).unwrap();

    let calls = runner.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["workspace-group", "list", "--json"]);
}
