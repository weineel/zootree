use serde_json::Value;
use zootree::core::cmux_layout::{
    default_cmux_anchor_layout, default_cmux_layout, default_cmux_repo_layout,
    render_cmux_anchor_layout, render_cmux_layout, render_cmux_repo_layout, CmuxLayoutVar,
};

fn vars() -> Vec<CmuxLayoutVar> {
    vec![
        CmuxLayoutVar {
            repo_name: "api".into(),
            worktree_path: "/tmp/fair-fox/api".into(),
            branch: "zootree/fair-fox".into(),
            workspace_name: "fair-fox".into(),
            workspace_dir: "/tmp/fair-fox".into(),
            lazygit_config: String::new(),
            overview_agent_command: String::new(),
            repo_agent_command: "claude --print 'Fix auth'".into(),
        },
        CmuxLayoutVar {
            repo_name: "web".into(),
            worktree_path: "/tmp/fair-fox/web".into(),
            branch: "zootree/fair-fox".into(),
            workspace_name: "fair-fox".into(),
            workspace_dir: "/tmp/fair-fox".into(),
            lazygit_config: String::new(),
            overview_agent_command: String::new(),
            repo_agent_command: String::new(),
        },
    ]
}

fn rendered_default_value() -> Value {
    let rendered = render_cmux_layout(default_cmux_layout(), &vars()).unwrap();
    serde_json::from_str(&rendered).unwrap()
}

#[test]
fn default_cmux_layout_is_valid_json() {
    let rendered = render_cmux_layout(default_cmux_layout(), &vars()).unwrap();
    serde_json::from_str::<Value>(&rendered).unwrap();
}

#[test]
fn default_layout_uses_repo_shell_tabs_without_lazygit() {
    let value = rendered_default_value();
    let children = value["children"].as_array().expect("root children");
    assert_eq!(children.len(), 2, "overview plus repo shell area");

    let shell_surfaces = children[1]["pane"]["surfaces"]
        .as_array()
        .expect("shell surfaces");
    assert_eq!(shell_surfaces.len(), 3);
    assert_eq!(shell_surfaces[0]["name"], "api");
    assert_eq!(shell_surfaces[1]["name"], "web");
    assert_eq!(shell_surfaces[2]["name"], "agent:api");

    let commands = collect_string_field(&value, "command");
    assert!(
        commands.iter().all(|command| !command.contains("lazygit")),
        "default cmux layout should not launch lazygit: {commands:?}"
    );
}

#[test]
fn repeat_per_repo_expands_once_per_repo() {
    let value = rendered_default_value();
    let commands = collect_string_field(&value, "command");
    let cwds = collect_string_field(&value, "cwd");

    assert!(commands.contains(&"zootree info fair-fox --watch".to_string()));
    assert!(cwds.contains(&"/tmp/fair-fox/api".to_string()));
    assert!(cwds.contains(&"/tmp/fair-fox/web".to_string()));
}

#[test]
fn empty_agent_command_surfaces_are_removed() {
    let value = rendered_default_value();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(&"zootree info fair-fox --watch".to_string()));
    assert!(commands.contains(&"claude --print 'Fix auth'".to_string()));
    assert!(!commands.contains(&String::new()));

    assert_no_empty_command(&value);
}

#[test]
fn repo_agent_command_is_preserved_for_single_repo_in_repo_area() {
    let one_repo = vec![vars().remove(0)];
    let rendered = render_cmux_layout(default_cmux_layout(), &one_repo).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(&"claude --print 'Fix auth'".to_string()));
}

#[test]
fn rendered_layout_has_no_empty_objects_or_unresolved_variables() {
    let value = rendered_default_value();
    assert_no_empty_object(&value);
    assert_no_unresolved_vars(&value);
}

#[test]
fn command_only_pane_with_empty_command_leaves_no_empty_wrapper() {
    let template = r#"{
  "children": [
    {
      "pane": {
        "surfaces": [
          {
            "type": "terminal",
            "command": "$repo_agent_command"
          }
        ]
      }
    }
  ]
}"#;
    let one_repo = vec![vars().remove(1)];
    let rendered = render_cmux_layout(template, &one_repo).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(
        value["children"].as_array().expect("root children").len(),
        0
    );
    assert_no_empty_object(&value);
}

#[test]
fn rendered_split_nodes_are_binary_and_have_split_ratio() {
    let value = rendered_default_value();
    assert_valid_cmux_split_tree(&value);
}

#[test]
fn split_nodes_with_more_than_two_children_are_folded() {
    let template = r#"{
  "direction": "horizontal",
  "children": [
    { "pane": { "surfaces": [{ "type": "terminal", "name": "one" }] } },
    { "pane": { "surfaces": [{ "type": "terminal", "name": "two" }] } },
    { "pane": { "surfaces": [{ "type": "terminal", "name": "three" }] } }
  ]
}"#;
    let rendered = render_cmux_layout(template, &vars()).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();

    assert_valid_cmux_split_tree(&value);
}

#[test]
fn anchor_layout_runs_info_and_multi_repo_agent() {
    let rendered = render_cmux_anchor_layout(
        default_cmux_anchor_layout(),
        &vars(),
        Some("codex --prompt 'Fix login'"),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");
    let cwds = collect_string_field(&value, "cwd");

    assert!(commands.contains(&"zootree info fair-fox --watch".to_string()));
    assert!(commands.contains(&"codex --prompt 'Fix login'".to_string()));
    assert!(cwds.contains(&"/tmp/fair-fox".to_string()));
    assert_no_empty_command(&value);
    assert_no_unresolved_vars(&value);
    assert_valid_cmux_split_tree(&value);
}

#[test]
fn anchor_layout_shell_quotes_info_workspace_name() {
    let mut vars = vars();
    vars[0].workspace_name = "fair fox'; touch /tmp/pwned; echo '".into();
    vars[1].workspace_name = vars[0].workspace_name.clone();
    let expected = shlex::try_join([
        "zootree",
        "info",
        vars[0].workspace_name.as_str(),
        "--watch",
    ])
    .unwrap();

    let rendered = render_cmux_anchor_layout(default_cmux_anchor_layout(), &vars, None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(&expected), "commands: {commands:?}");
    assert_ne!(
        expected,
        "zootree info fair fox'; touch /tmp/pwned; echo ' --watch"
    );
    assert_eq!(
        shlex::split(&expected).unwrap(),
        vec![
            "zootree",
            "info",
            "fair fox'; touch /tmp/pwned; echo '",
            "--watch"
        ]
    );
}

#[test]
fn anchor_layout_without_multi_repo_agent_uses_shell_on_right() {
    let rendered = render_cmux_anchor_layout(default_cmux_anchor_layout(), &vars(), None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");
    let cwds = collect_string_field(&value, "cwd");

    assert!(commands.contains(&"zootree info fair-fox --watch".to_string()));
    assert!(!commands.iter().any(|command| command.contains("codex")));
    assert!(cwds.contains(&"/tmp/fair-fox".to_string()));
    assert_no_empty_command(&value);
}

#[test]
fn repo_layout_runs_lazygit_and_single_repo_agent() {
    let repo = vars().remove(0);
    let rendered = render_cmux_repo_layout(
        default_cmux_repo_layout(),
        &repo,
        Some("codex --prompt 'Fix login'"),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");
    let cwds = collect_string_field(&value, "cwd");

    assert!(commands.contains(&"lazygit -p /tmp/fair-fox/api".to_string()));
    assert!(commands.contains(&"codex --prompt 'Fix login'".to_string()));
    assert!(cwds.iter().any(|cwd| cwd == "/tmp/fair-fox/api"));
    assert_no_empty_command(&value);
    assert_no_unresolved_vars(&value);
    assert_valid_cmux_split_tree(&value);
}

#[test]
fn repo_layout_without_agent_keeps_shell_bottom() {
    let repo = vars().remove(0);
    let rendered = render_cmux_repo_layout(default_cmux_repo_layout(), &repo, None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(&"lazygit -p /tmp/fair-fox/api".to_string()));
    assert!(!commands.iter().any(|command| command.contains("codex")));
    assert_no_empty_command(&value);
}

#[test]
fn repo_layout_passes_lazygit_config_when_present() {
    let mut repo = vars().remove(0);
    repo.lazygit_config = "/tmp/lazygit.yml".into();

    let rendered = render_cmux_repo_layout(default_cmux_repo_layout(), &repo, None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(&"lazygit -p /tmp/fair-fox/api -ucf /tmp/lazygit.yml".to_string()));
}

#[test]
fn repo_layout_quotes_lazygit_paths_with_spaces() {
    let mut repo = vars().remove(0);
    repo.worktree_path = "/tmp/fair fox/api service".into();
    repo.lazygit_config = "/tmp/cmux configs/lazygit user.yml".into();
    let expected = shlex::try_join([
        "lazygit",
        "-p",
        repo.worktree_path.as_str(),
        "-ucf",
        repo.lazygit_config.as_str(),
    ])
    .unwrap();

    let rendered = render_cmux_repo_layout(default_cmux_repo_layout(), &repo, None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    let commands = collect_string_field(&value, "command");

    assert!(commands.contains(&expected));
    assert_eq!(
        shlex::split(&expected).unwrap(),
        vec![
            "lazygit",
            "-p",
            "/tmp/fair fox/api service",
            "-ucf",
            "/tmp/cmux configs/lazygit user.yml"
        ]
    );
}

fn collect_string_field(value: &Value, field: &str) -> Vec<String> {
    let mut values = Vec::new();
    collect_string_field_into(value, field, &mut values);
    values
}

fn collect_string_field_into(value: &Value, field: &str, values: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(s) = map.get(field).and_then(Value::as_str) {
                values.push(s.to_string());
            }
            for child in map.values() {
                collect_string_field_into(child, field, values);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_string_field_into(item, field, values);
            }
        }
        _ => {}
    }
}

fn assert_no_empty_command(value: &Value) {
    match value {
        Value::Object(map) => {
            assert_ne!(map.get("command").and_then(Value::as_str), Some(""));
            for child in map.values() {
                assert_no_empty_command(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_no_empty_command(item);
            }
        }
        _ => {}
    }
}

fn assert_no_empty_object(value: &Value) {
    match value {
        Value::Object(map) => {
            assert!(!map.is_empty(), "empty object found in {value}");
            for child in map.values() {
                assert_no_empty_object(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_no_empty_object(item);
            }
        }
        _ => {}
    }
}

fn assert_no_unresolved_vars(value: &Value) {
    match value {
        Value::String(s) => assert!(!s.contains('$'), "unresolved variable in {s:?}"),
        Value::Object(map) => {
            for child in map.values() {
                assert_no_unresolved_vars(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_no_unresolved_vars(item);
            }
        }
        _ => {}
    }
}

fn assert_valid_cmux_split_tree(value: &Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("direction") {
                assert!(
                    map.get("split").and_then(Value::as_f64).is_some(),
                    "split node missing split ratio: {value}"
                );
                let children = map
                    .get("children")
                    .and_then(Value::as_array)
                    .expect("split node children");
                assert_eq!(
                    children.len(),
                    2,
                    "split node must have two children: {value}"
                );
            }
            for child in map.values() {
                assert_valid_cmux_split_tree(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_valid_cmux_split_tree(item);
            }
        }
        _ => {}
    }
}
