use zootree::core::layout::{LayoutRenderer, LayoutVar};

#[test]
fn test_variable_replacement() {
    let template = r#"tab name="$repo_name" {
    pane cwd="$worktree_path"
}"#;
    let vars = LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/home/user/ws/calm-river/frontend".into(),
        branch: "zootree/calm-river".into(),
        workspace_name: "calm-river".into(),
        workspace_dir: "/home/user/ws/calm-river".into(),
        lazygit_config: "".into(),
        overview_agent_cli: "".into(),
        repo_agent_cli: "".into(),
    };
    let result = LayoutRenderer::replace_vars(template, &vars);
    assert!(result.contains(r#"name="frontend""#));
    assert!(result.contains(r#"cwd="/home/user/ws/calm-river/frontend""#));
}

#[test]
fn test_empty_lazygit_config_removes_ucf_arg() {
    let template = r#"pane command="lazygit" {
    args "-p" "$worktree_path" "-ucf" "$lazygit_config"
}"#;
    let vars = LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/home/user/ws/frontend".into(),
        branch: "zootree/test".into(),
        workspace_name: "test".into(),
        workspace_dir: "/home/user/ws".into(),
        lazygit_config: "".into(),
        overview_agent_cli: "".into(),
        repo_agent_cli: "".into(),
    };
    let result = LayoutRenderer::replace_vars(template, &vars);
    assert!(!result.contains("-ucf"));
    assert!(!result.contains("$lazygit_config"));
}

#[test]
fn test_nonempty_lazygit_config_keeps_ucf_arg() {
    let template = r#"pane command="lazygit" {
    args "-p" "$worktree_path" "-ucf" "$lazygit_config"
}"#;
    let vars = LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/home/user/ws/frontend".into(),
        branch: "zootree/test".into(),
        workspace_name: "test".into(),
        workspace_dir: "/home/user/ws".into(),
        lazygit_config: "/home/user/.lazygit.yml".into(),
        overview_agent_cli: "".into(),
        repo_agent_cli: "".into(),
    };
    let result = LayoutRenderer::replace_vars(template, &vars);
    assert!(result.contains(r#""-ucf" "/home/user/.lazygit.yml""#));
}

#[test]
fn test_repeat_per_repo() {
    let template = r#"layout {
    tab name="overview" {
        pane command="zootree"
    }

    // @repeat-per-repo
    tab name="$repo_name" {
        pane cwd="$worktree_path"
    }
}"#;
    let repos = vec![
        LayoutVar {
            repo_name: "frontend".into(),
            worktree_path: "/ws/frontend".into(),
            branch: "zootree/test".into(),
            workspace_name: "test".into(),
            workspace_dir: "/ws".into(),
            lazygit_config: "".into(),
            overview_agent_cli: "".into(),
            repo_agent_cli: "".into(),
        },
        LayoutVar {
            repo_name: "backend".into(),
            worktree_path: "/ws/backend".into(),
            branch: "zootree/test".into(),
            workspace_name: "test".into(),
            workspace_dir: "/ws".into(),
            lazygit_config: "".into(),
            overview_agent_cli: "".into(),
            repo_agent_cli: "".into(),
        },
    ];
    let result = LayoutRenderer::render(template, &repos);
    assert!(result.contains(r#"name="overview""#));
    assert!(result.contains(r#"name="frontend""#));
    assert!(result.contains(r#"name="backend""#));
    assert!(result.contains(r#"cwd="/ws/frontend""#));
    assert!(result.contains(r#"cwd="/ws/backend""#));
    assert!(!result.contains("@repeat-per-repo"));
    assert!(!result.contains("$repo_name"));
}

#[test]
fn default_layout_overview_uses_info_watch() {
    let template = LayoutRenderer::default_layout();
    assert!(
        template.contains(r#""info" "$workspace_name" "--watch""#),
        "default layout should use `zootree info <name> --watch` in overview\n---\n{}",
        template
    );
    assert!(
        !template.contains(r#""list" "--status" "in_progress""#),
        "default layout should no longer spawn list in overview\n---\n{}",
        template
    );
}

#[test]
fn default_layout_defines_tab_template_for_new_tabs() {
    let template = LayoutRenderer::default_layout();
    assert!(
        template.contains("default_tab_template"),
        "default layout should define default_tab_template so manually opened tabs inherit chrome\n---\n{}",
        template
    );
    assert!(
        template.contains("children"),
        "default_tab_template should include children placeholder\n---\n{}",
        template
    );
}

#[test]
fn default_layout_info_args_expanded_on_render() {
    let template = LayoutRenderer::default_layout();
    let vars = vec![LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/ws/calm-river/frontend".into(),
        branch: "zootree/calm-river".into(),
        workspace_name: "calm-river".into(),
        workspace_dir: "/ws/calm-river".into(),
        lazygit_config: "".into(),
        overview_agent_cli: "".into(),
        repo_agent_cli: "".into(),
    }];
    let rendered = LayoutRenderer::render(template, &vars);
    assert!(
        rendered.contains(r#""info" "calm-river" "--watch""#),
        "expected $workspace_name to expand\n---\n{}",
        rendered
    );
}

#[test]
fn test_overview_agent_cli_placeholder_substituted() {
    let template = r#"pane cwd="$workspace_dir" $overview_agent_cli"#;
    let vars = LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/ws/frontend".into(),
        branch: "zootree/test".into(),
        workspace_name: "test".into(),
        workspace_dir: "/ws".into(),
        lazygit_config: "".into(),
        overview_agent_cli: r#"command="claude" {
    args "--" "hello"
}"#
        .into(),
        repo_agent_cli: "".into(),
    };
    let result = LayoutRenderer::replace_vars(template, &vars);
    assert!(
        result.contains(r#"cwd="/ws""#),
        "workspace_dir replaced: {}",
        result
    );
    assert!(
        result.contains(r#"command="claude""#),
        "agent_cli kdl injected: {}",
        result
    );
    assert!(
        result.contains(r#"args "--" "hello""#),
        "agent args present: {}",
        result
    );
    assert!(!result.contains("$overview_agent_cli"));
}

#[test]
fn test_empty_agent_cli_leaves_layout_intact() {
    let template = r#"pane cwd="$worktree_path" $repo_agent_cli"#;
    let vars = LayoutVar {
        repo_name: "frontend".into(),
        worktree_path: "/ws/frontend".into(),
        branch: "zootree/test".into(),
        workspace_name: "test".into(),
        workspace_dir: "/ws".into(),
        lazygit_config: "".into(),
        overview_agent_cli: "".into(),
        repo_agent_cli: "".into(),
    };
    let result = LayoutRenderer::replace_vars(template, &vars);
    assert!(!result.contains("$repo_agent_cli"));
    assert!(!result.contains("command=\"claude\""));
    assert!(result.contains(r#"cwd="/ws/frontend""#));
}
