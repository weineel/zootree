use zootree::config::global::ZellijConfig;
use zootree::config::workspace::WorkspaceConfig;
use zootree::core::layout::build_prompt;

fn make_workspace(title: &str, description: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: title.into(),
        name: "test-ws".into(),
        description: description.into(),
        branch: "zootree/test".into(),
        workspace_dir: "/tmp/ws".into(),
        created_at: "2026-05-12T00:00:00+08:00".into(),
        zellij: ZellijConfig::default(),
        repos: Vec::new(),
        events: Vec::new(),
    }
}

#[test]
fn build_prompt_uses_title_only_when_description_empty() {
    let ws = make_workspace("Add login flow", "");
    assert_eq!(build_prompt(&ws), "Add login flow");
}

#[test]
fn build_prompt_joins_title_and_description_with_newline() {
    let ws = make_workspace("Add login", "Implement OAuth2");
    assert_eq!(build_prompt(&ws), "Add login\nImplement OAuth2");
}

use zootree::core::layout::build_agent_cli_kdl;

#[test]
fn build_agent_cli_kdl_basic_with_prompt() {
    let kdl = build_agent_cli_kdl("claude -- $prompt", "hello").unwrap();
    assert_eq!(
        kdl,
        r#"command="claude" {
    args "--" "hello"
}"#
    );
}

#[test]
fn build_agent_cli_kdl_no_args_block_when_only_command() {
    let kdl = build_agent_cli_kdl("claude", "ignored").unwrap();
    assert_eq!(kdl, r#"command="claude""#);
}

#[test]
fn build_agent_cli_kdl_ignores_prompt_when_no_placeholder() {
    let kdl = build_agent_cli_kdl("gemini chat", "ignored").unwrap();
    assert_eq!(
        kdl,
        r#"command="gemini" {
    args "chat"
}"#
    );
}

#[test]
fn build_agent_cli_kdl_embedded_prompt_token() {
    let kdl = build_agent_cli_kdl("claude --prompt=$prompt", "hello world").unwrap();
    assert_eq!(
        kdl,
        r#"command="claude" {
    args "--prompt=hello world"
}"#
    );
}

#[test]
fn build_agent_cli_kdl_escapes_double_quote_in_prompt() {
    let kdl = build_agent_cli_kdl("claude -- $prompt", r#"say "hi""#).unwrap();
    assert!(kdl.contains(r#""--" "say \"hi\"""#), "got: {}", kdl);
}

#[test]
fn build_agent_cli_kdl_escapes_backslash_in_prompt() {
    let kdl = build_agent_cli_kdl("claude -- $prompt", r"path\to\file").unwrap();
    assert!(kdl.contains(r#""--" "path\\to\\file""#), "got: {}", kdl);
}

#[test]
fn build_agent_cli_kdl_escapes_newline_in_prompt() {
    let kdl = build_agent_cli_kdl("claude -- $prompt", "line1\nline2").unwrap();
    assert!(kdl.contains(r#""--" "line1\nline2""#), "got: {}", kdl);
}

#[test]
fn build_agent_cli_kdl_empty_template_errors() {
    let err = build_agent_cli_kdl("", "any").unwrap_err();
    let msg = format!("{:#}", err);
    assert!(msg.contains("agent_cli is empty"), "got: {}", msg);
}

#[test]
fn build_agent_cli_kdl_unclosed_quote_errors() {
    let err = build_agent_cli_kdl(r#"claude "unclosed"#, "any").unwrap_err();
    let msg = format!("{:#}", err);
    assert!(msg.contains("failed to parse agent_cli"), "got: {}", msg);
}
