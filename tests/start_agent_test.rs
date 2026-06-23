use std::collections::BTreeMap;
use zootree::config::global::ZellijConfig;
use zootree::config::workspace::{RepoEntry, WorkspaceConfig};
use zootree::core::layout::{build_agent_cli_kdl, build_prompt, LayoutRenderer, LayoutVar};

fn make_workspace(repos: Vec<&str>) -> WorkspaceConfig {
    WorkspaceConfig {
        title: "Add login flow".into(),
        name: "calm-river".into(),
        description: "Implement OAuth2".into(),
        branch: "zootree/calm-river".into(),
        workspace_dir: "/ws/calm-river".into(),
        created_at: "2026-05-12T00:00:00+08:00".into(),
        agent_cli: None,
        zellij: ZellijConfig::default(),
        repos: repos
            .into_iter()
            .map(|n| RepoEntry {
                name: n.into(),
                target_branch: Some("main".into()),
            })
            .collect(),
        events: Vec::new(),
    }
}

fn render_with_rule(
    workspace: &WorkspaceConfig,
    run_agent: Option<Option<&str>>,
    agent_cli: Option<&str>,
    alias_map: &BTreeMap<String, String>,
) -> anyhow::Result<String> {
    let agent_cli_tpl: Option<String> = match run_agent {
        None => None,
        Some(value) => {
            let raw = match value {
                Some(s) if !s.is_empty() => s,
                _ => agent_cli.ok_or_else(|| {
                    anyhow::anyhow!("--run-agent requires agent_cli in global config")
                })?,
            };
            let resolved = zootree::core::layout::resolve_agent_cli(raw, alias_map);
            Some(resolved.to_string())
        }
    };

    let (overview_kdl, repo_kdl_for_first) = match agent_cli_tpl.as_deref() {
        None => (String::new(), String::new()),
        Some(tpl) => {
            let prompt = build_prompt(workspace);
            let kdl = build_agent_cli_kdl(tpl, &prompt)?;
            if workspace.repos.len() == 1 {
                (String::new(), kdl)
            } else {
                (kdl, String::new())
            }
        }
    };

    let mut vars = Vec::new();
    for (i, repo) in workspace.repos.iter().enumerate() {
        vars.push(LayoutVar {
            repo_name: repo.name.clone(),
            worktree_path: format!("{}/{}", workspace.workspace_dir, repo.name),
            branch: workspace.branch.clone(),
            workspace_name: workspace.name.clone(),
            workspace_dir: workspace.workspace_dir.clone(),
            lazygit_config: String::new(),
            overview_agent_cli: overview_kdl.clone(),
            repo_agent_cli: if i == 0 {
                repo_kdl_for_first.clone()
            } else {
                String::new()
            },
        });
    }

    Ok(LayoutRenderer::render(
        LayoutRenderer::default_layout(),
        &vars,
    ))
}

fn split_overview_and_repo_tabs(rendered: &str) -> (String, String) {
    let first_tab_start = rendered
        .find(r#"tab name="overview""#)
        .expect("overview tab missing");
    let after_first = &rendered[first_tab_start..];
    let second_tab_rel = after_first[1..]
        .find(r#"tab name=""#)
        .expect("repo tab missing");
    let split_at = first_tab_start + 1 + second_tab_rel;
    (
        rendered[..split_at].to_string(),
        rendered[split_at..].to_string(),
    )
}

#[test]
fn run_agent_with_one_repo_injects_into_repo_pane_only() {
    let ws = make_workspace(vec!["frontend"]);
    let map = BTreeMap::new();
    let rendered = render_with_rule(
        &ws,
        Some(Some("")),
        Some("claude --dangerously-skip-permissions -- $prompt"),
        &map,
    )
    .unwrap();

    let (overview, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        !overview.contains(r#"command="claude""#),
        "overview should NOT contain agent: {}",
        overview
    );
    assert!(
        repo_section.contains(r#"command="claude""#),
        "repo section should contain agent: {}",
        repo_section
    );
    assert!(
        repo_section.contains(r#""--dangerously-skip-permissions""#),
        "agent args present in repo section: {}",
        repo_section
    );
    assert!(
        repo_section.contains(r#""Add login flow\nImplement OAuth2""#),
        "prompt joined with newline (escaped): {}",
        repo_section
    );
}

#[test]
fn run_agent_with_two_repos_injects_into_overview_only() {
    let ws = make_workspace(vec!["frontend", "backend"]);
    let map = BTreeMap::new();
    let rendered = render_with_rule(&ws, Some(Some("")), Some("claude -- $prompt"), &map).unwrap();

    let (overview, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        overview.contains(r#"command="claude""#),
        "overview contains agent: {}",
        overview
    );
    assert!(
        !repo_section.contains(r#"command="claude""#),
        "no repo tab contains agent: {}",
        repo_section
    );
}

#[test]
fn no_run_agent_keeps_layout_clean() {
    let ws = make_workspace(vec!["frontend"]);
    let map = BTreeMap::new();
    let rendered = render_with_rule(&ws, None, Some("claude -- $prompt"), &map).unwrap();

    assert!(!rendered.contains(r#"command="claude""#));
    assert!(!rendered.contains("$overview_agent_cli"));
    assert!(!rendered.contains("$repo_agent_cli"));
}

#[test]
fn run_agent_without_agent_cli_errors() {
    let ws = make_workspace(vec!["frontend"]);
    let map = BTreeMap::new();
    let err = render_with_rule(&ws, Some(Some("")), None, &map).unwrap_err();
    let msg = format!("{:#}", err);
    assert!(
        msg.contains("agent_cli"),
        "error mentions agent_cli: {}",
        msg
    );
}

#[test]
fn agent_cli_with_embedded_prompt_token() {
    let ws = make_workspace(vec!["frontend"]);
    let map = BTreeMap::new();
    let rendered =
        render_with_rule(&ws, Some(Some("")), Some("claude --prompt=$prompt"), &map).unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#""--prompt=Add login flow\nImplement OAuth2""#),
        "embedded prompt substituted: {}",
        repo_section
    );
}

fn alias_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn run_agent_with_alias_resolves_template() {
    let ws = make_workspace(vec!["frontend"]);
    let map = alias_map(&[("safe", "claude -- $prompt")]);
    let rendered = render_with_rule(&ws, Some(Some("safe")), None, &map).unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#"command="claude""#),
        "alias 'safe' resolved to claude template: {}",
        repo_section
    );
}

#[test]
fn run_agent_with_unknown_alias_falls_back_to_literal() {
    let ws = make_workspace(vec!["frontend"]);
    let map = alias_map(&[("safe", "ignored -- $prompt")]);
    let rendered = render_with_rule(&ws, Some(Some("gemini chat -- $prompt")), None, &map).unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#"command="gemini""#),
        "literal command used: {}",
        repo_section
    );
    assert!(
        !repo_section.contains("ignored"),
        "alias 'safe' value not used: {}",
        repo_section
    );
}

#[test]
fn run_agent_alias_template_takes_precedence_over_agent_cli() {
    let ws = make_workspace(vec!["frontend"]);
    let map = alias_map(&[("bar", "bar -- $prompt")]);
    let rendered = render_with_rule(&ws, Some(Some("bar")), Some("foo -- $prompt"), &map).unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#"command="bar""#),
        "alias takes precedence: {}",
        repo_section
    );
    assert!(
        !repo_section.contains(r#"command="foo""#),
        "agent_cli not used: {}",
        repo_section
    );
}

#[test]
fn run_agent_bare_with_agent_cli_uses_default() {
    let ws = make_workspace(vec!["frontend"]);
    let map: BTreeMap<String, String> = BTreeMap::new();
    let rendered = render_with_rule(&ws, Some(Some("")), Some("claude -- $prompt"), &map).unwrap();

    let (_, repo_section) = split_overview_and_repo_tabs(&rendered);
    assert!(
        repo_section.contains(r#"command="claude""#),
        "bare flag uses agent_cli: {}",
        repo_section
    );
}
