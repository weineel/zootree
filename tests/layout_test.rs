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
        },
        LayoutVar {
            repo_name: "backend".into(),
            worktree_path: "/ws/backend".into(),
            branch: "zootree/test".into(),
            workspace_name: "test".into(),
            workspace_dir: "/ws".into(),
            lazygit_config: "".into(),
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
