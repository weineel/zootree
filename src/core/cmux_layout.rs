use anyhow::Result;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxLayoutVar {
    pub repo_name: String,
    pub worktree_path: String,
    pub branch: String,
    pub workspace_name: String,
    pub workspace_dir: String,
    pub lazygit_config: String,
    pub overview_agent_command: String,
    pub repo_agent_command: String,
}

pub fn default_cmux_layout() -> &'static str {
    r#"{
  "direction": "horizontal",
  "split": 0.38,
  "children": [
    {
      "pane": {
        "surfaces": [
          {
            "type": "terminal",
            "name": "info",
            "command": "zootree info $workspace_name --watch",
            "cwd": "$workspace_dir",
            "focus": true
          },
          {
            "type": "terminal",
            "name": "agent",
            "command": "$overview_agent_command",
            "cwd": "$workspace_dir"
          }
        ]
      }
    },
    {
      "pane": {
        "surfaces": [
          {
            "zootree_repeat_per_repo": {
              "type": "terminal",
              "name": "$repo_name",
              "cwd": "$worktree_path"
            }
          },
          {
            "zootree_repeat_per_repo": {
              "type": "terminal",
              "name": "agent:$repo_name",
              "command": "$repo_agent_command",
              "cwd": "$worktree_path"
            }
          }
        ]
      }
    }
  ]
}"#
}

pub fn render_cmux_layout(template: &str, repos: &[CmuxLayoutVar]) -> Result<String> {
    let mut value: Value = serde_json::from_str(template)?;
    expand_value(&mut value, repos, repos.first())?;
    prune_empty(&mut value);
    normalize_layout_tree(&mut value);
    Ok(serde_json::to_string(&value)?)
}

fn expand_value(
    value: &mut Value,
    repos: &[CmuxLayoutVar],
    workspace_vars: Option<&CmuxLayoutVar>,
) -> Result<()> {
    match value {
        Value::Object(map) => {
            if let Some(repeat) = map.remove("zootree_repeat_per_repo") {
                let expanded = repos
                    .iter()
                    .map(|repo| {
                        let mut item = repeat.clone();
                        expand_value(&mut item, repos, Some(repo))?;
                        Ok(item)
                    })
                    .collect::<Result<Vec<_>>>()?;
                *value = Value::Array(expanded);
                return Ok(());
            }

            for child in map.values_mut() {
                expand_value(child, repos, workspace_vars)?;
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                expand_value(item, repos, workspace_vars)?;
            }
            flatten_arrays(items);
        }
        Value::String(s) => {
            if let Some(vars) = workspace_vars {
                *s = replace_vars(s, vars);
            }
        }
        _ => {}
    }

    Ok(())
}

fn flatten_arrays(items: &mut Vec<Value>) {
    let mut flattened = Vec::new();
    for item in std::mem::take(items) {
        match item {
            Value::Array(nested) => flattened.extend(nested),
            other => flattened.push(other),
        }
    }
    *items = flattened;
}

fn replace_vars(input: &str, vars: &CmuxLayoutVar) -> String {
    input
        .replace("$workspace_name", &vars.workspace_name)
        .replace("$workspace_dir", &vars.workspace_dir)
        .replace("$repo_name", &vars.repo_name)
        .replace("$worktree_path", &vars.worktree_path)
        .replace("$branch", &vars.branch)
        .replace("$lazygit_config", &vars.lazygit_config)
        .replace("$overview_agent_command", &vars.overview_agent_command)
        .replace("$repo_agent_command", &vars.repo_agent_command)
}

fn prune_empty(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            if map
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(str::is_empty)
            {
                return true;
            }

            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let remove = map.get_mut(&key).is_some_and(prune_empty);
                if remove {
                    map.remove(&key);
                }
            }

            if map.is_empty() {
                return true;
            }

            has_empty_array(map, "surfaces") || has_empty_array(map, "children")
        }
        Value::Array(items) => {
            let mut retained = Vec::new();
            for mut item in std::mem::take(items) {
                if !prune_empty(&mut item) {
                    retained.push(item);
                }
            }
            *items = retained;
            false
        }
        _ => false,
    }
}

fn has_empty_array(map: &serde_json::Map<String, Value>, key: &str) -> bool {
    map.get(key)
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
}

fn normalize_layout_tree(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for child in map.values_mut() {
                normalize_layout_tree(child);
            }

            normalize_split_node(value);
        }
        Value::Array(items) => {
            for item in items {
                normalize_layout_tree(item);
            }
        }
        _ => {}
    }
}

fn normalize_split_node(value: &mut Value) {
    let Value::Object(map) = value else {
        return;
    };
    if !map.contains_key("direction") || !map.contains_key("children") {
        return;
    }
    let direction = map
        .get("direction")
        .cloned()
        .unwrap_or_else(|| Value::String("horizontal".into()));
    let split = map
        .get("split")
        .cloned()
        .unwrap_or_else(default_split_value);

    let Some(Value::Array(children)) = map.get_mut("children") else {
        return;
    };

    match children.len() {
        0 => {}
        1 => {
            let mut child = children.remove(0);
            normalize_layout_tree(&mut child);
            *value = child;
        }
        2 => {
            let map = value.as_object_mut().expect("split node remains object");
            map.entry("split").or_insert_with(default_split_value);
        }
        _ => {
            let folded = fold_children(direction.clone(), split.clone(), std::mem::take(children));
            *children = match folded {
                Value::Object(mut folded_map) => folded_map
                    .remove("children")
                    .and_then(|children| match children {
                        Value::Array(children) => Some(children),
                        _ => None,
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            map.insert("split".into(), split);
            map.insert("direction".into(), direction);
        }
    }
}

fn fold_children(direction: Value, split: Value, mut children: Vec<Value>) -> Value {
    debug_assert!(children.len() >= 2);

    if children.len() == 2 {
        return split_node(direction, split, children);
    }

    let first = children.remove(0);
    let rest = fold_children(direction.clone(), split.clone(), children);
    split_node(direction, split, vec![first, rest])
}

fn split_node(direction: Value, split: Value, children: Vec<Value>) -> Value {
    let mut map = Map::new();
    map.insert("direction".into(), direction);
    map.insert("split".into(), split);
    map.insert("children".into(), Value::Array(children));
    Value::Object(map)
}

fn default_split_value() -> Value {
    Value::from(0.5)
}
