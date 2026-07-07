use std::collections::BTreeMap;
use zootree::config::global::MultiplexerConfig;
use zootree::config::workspace::WorkspaceConfig;
use zootree::core::layout::{build_prompt, resolve_agent_cli};

fn make_workspace(title: &str, description: &str) -> WorkspaceConfig {
    WorkspaceConfig {
        title: title.into(),
        name: "test-ws".into(),
        description: description.into(),
        branch: "zootree/test".into(),
        workspace_dir: "/tmp/ws".into(),
        created_at: "2026-05-12T00:00:00+08:00".into(),
        agent_cli: None,
        multiplexer: MultiplexerConfig::default(),
        multiplexer_state: Default::default(),
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

use zootree::core::layout::{build_agent_cli_display, AliasInfo};

#[test]
fn build_agent_cli_display_returns_none_when_unset() {
    let ws = make_workspace("Hello", "");
    assert!(build_agent_cli_display(None, &BTreeMap::new(), &ws).is_none());
}

#[test]
fn build_agent_cli_display_substitutes_single_line_prompt() {
    let ws = make_workspace("Add login", "");
    let out = build_agent_cli_display(Some("claude --skip -- $prompt"), &BTreeMap::new(), &ws)
        .expect("Some")
        .expect("Ok");
    assert!(out.command.contains("claude"), "got: {}", out.command);
    assert!(out.command.contains("--skip"), "got: {}", out.command);
    assert!(
        out.command.contains("'Add login'"),
        "expected single-quoted prompt, got: {}",
        out.command
    );
    assert!(
        out.alias.is_none(),
        "expected no alias, got: {:?}",
        out.alias
    );
}

#[test]
fn build_agent_cli_display_handles_multiline_prompt() {
    let ws = make_workspace("Add login", "Implement OAuth2");
    let out = build_agent_cli_display(Some("claude --skip -- $prompt"), &BTreeMap::new(), &ws)
        .expect("Some")
        .expect("Ok");
    // build_prompt joins title + description with '\n'.
    // shlex::try_join uses POSIX single-quoting which preserves the literal newline byte.
    assert!(
        out.command.contains("Add login\nImplement OAuth2"),
        "got: {:?}",
        out.command
    );
}

#[test]
fn build_agent_cli_display_returns_err_on_invalid_template() {
    let ws = make_workspace("Hello", "");
    let unclosed = "claude 'unclosed";
    let result = build_agent_cli_display(Some(unclosed), &BTreeMap::new(), &ws).expect("Some");
    assert!(
        result.is_err(),
        "expected Err for unclosed quote, got: {:?}",
        result.map(|d| d.command)
    );
}

#[test]
fn build_agent_cli_display_returns_err_on_empty_template() {
    let ws = make_workspace("Hello", "");
    let result = build_agent_cli_display(Some("   "), &BTreeMap::new(), &ws).expect("Some");
    assert!(
        result.is_err(),
        "expected Err for empty template, got: {:?}",
        result.map(|d| d.command)
    );
}

use zootree::core::layout::{build_agent_cli_command, build_agent_cli_kdl};

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

#[test]
fn build_agent_cli_command_shell_quotes_prompt() {
    let command = build_agent_cli_command("claude -- $prompt", "hello world").unwrap();
    assert_eq!(command, "claude -- 'hello world'");
}

#[test]
fn build_agent_cli_command_handles_multiline_prompt() {
    let command = build_agent_cli_command("claude -- $prompt", "line1\nline2").unwrap();
    assert!(command.contains("line1\nline2"), "got: {:?}", command);
}

#[test]
fn build_agent_cli_command_empty_template_errors() {
    let err = build_agent_cli_command("   ", "any").unwrap_err();
    let msg = format!("{:#}", err);
    assert!(msg.contains("agent_cli is empty"), "got: {}", msg);
}

#[test]
fn build_agent_cli_command_unclosed_quote_errors() {
    let err = build_agent_cli_command(r#"claude "unclosed"#, "any").unwrap_err();
    let msg = format!("{:#}", err);
    assert!(msg.contains("failed to parse agent_cli"), "got: {}", msg);
}

#[test]
fn resolve_returns_alias_value_when_key_matches() {
    let mut map = BTreeMap::new();
    map.insert("safe".to_string(), "claude -- $prompt".to_string());
    assert_eq!(resolve_agent_cli("safe", &map), "claude -- $prompt");
}

#[test]
fn resolve_returns_input_when_key_missing() {
    let mut map = BTreeMap::new();
    map.insert("safe".to_string(), "claude -- $prompt".to_string());
    assert_eq!(
        resolve_agent_cli("gemini chat -- $prompt", &map),
        "gemini chat -- $prompt"
    );
}

#[test]
fn resolve_returns_input_with_empty_alias_map() {
    let map = BTreeMap::new();
    assert_eq!(resolve_agent_cli("anything", &map), "anything");
}

#[test]
fn resolve_does_not_chain_aliases() {
    let mut map = BTreeMap::new();
    map.insert("a".to_string(), "b".to_string());
    map.insert("b".to_string(), "real -- $prompt".to_string());
    assert_eq!(resolve_agent_cli("a", &map), "b");
}

#[test]
fn build_agent_cli_display_resolves_alias_and_reports_provenance() {
    let ws = make_workspace("Add login", "");
    let mut alias_map = BTreeMap::new();
    alias_map.insert("safe".to_string(), "claude --skip -- $prompt".to_string());

    let out = build_agent_cli_display(Some("safe"), &alias_map, &ws)
        .expect("Some")
        .expect("Ok");

    assert_eq!(
        out.alias,
        Some(AliasInfo {
            name: "safe".to_string(),
            template: "claude --skip -- $prompt".to_string(),
        }),
    );
    assert!(out.command.contains("claude"), "got: {}", out.command);
    assert!(out.command.contains("--skip"), "got: {}", out.command);
    assert!(
        out.command.contains("'Add login'"),
        "expected prompt expansion, got: {}",
        out.command
    );
}

#[test]
fn build_agent_cli_display_no_alias_when_tpl_is_literal() {
    let ws = make_workspace("Hello", "");
    let mut alias_map = BTreeMap::new();
    alias_map.insert("safe".to_string(), "claude -- $prompt".to_string());

    let out = build_agent_cli_display(Some("gemini chat -- $prompt"), &alias_map, &ws)
        .expect("Some")
        .expect("Ok");

    assert!(out.alias.is_none(), "got: {:?}", out.alias);
    assert!(out.command.contains("gemini"), "got: {}", out.command);
}

#[test]
fn build_agent_cli_display_alias_with_invalid_template_errors() {
    let ws = make_workspace("Hello", "");
    let mut alias_map = BTreeMap::new();
    alias_map.insert("broken".to_string(), "claude 'unclosed".to_string());

    let result = build_agent_cli_display(Some("broken"), &alias_map, &ws).expect("Some");
    assert!(
        result.is_err(),
        "expected Err for alias pointing at invalid template, got: {:?}",
        result.map(|d| d.command)
    );
}

#[test]
fn build_agent_cli_display_alias_with_empty_template_errors() {
    let ws = make_workspace("Hello", "");
    let mut alias_map = BTreeMap::new();
    alias_map.insert("empty".to_string(), "   ".to_string());

    let result = build_agent_cli_display(Some("empty"), &alias_map, &ws).expect("Some");
    assert!(
        result.is_err(),
        "expected Err for alias pointing at empty template, got: {:?}",
        result.map(|d| d.command)
    );
}
