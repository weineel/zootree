pub struct LayoutVar {
    pub repo_name: String,
    pub worktree_path: String,
    pub branch: String,
    pub workspace_name: String,
    pub workspace_dir: String,
    pub lazygit_config: String,
}

pub struct LayoutRenderer;

impl LayoutRenderer {
    pub fn replace_vars(template: &str, vars: &LayoutVar) -> String {
        let mut result = template.to_string();

        // Handle empty lazygit_config: remove "-ucf" "$lazygit_config" pair
        if vars.lazygit_config.is_empty() {
            result = result.replace(r#" "-ucf" "$lazygit_config""#, "");
        }

        result = result.replace("$repo_name", &vars.repo_name);
        result = result.replace("$worktree_path", &vars.worktree_path);
        result = result.replace("$branch", &vars.branch);
        result = result.replace("$workspace_name", &vars.workspace_name);
        result = result.replace("$workspace_dir", &vars.workspace_dir);
        result = result.replace("$lazygit_config", &vars.lazygit_config);
        result
    }

    pub fn render(template: &str, repos: &[LayoutVar]) -> String {
        let marker = "// @repeat-per-repo";
        let Some(marker_pos) = template.find(marker) else {
            if let Some(vars) = repos.first() {
                return Self::replace_vars(template, vars);
            }
            return template.to_string();
        };

        let before_marker = &template[..marker_pos];
        let after_marker = &template[marker_pos + marker.len()..];
        let after_marker = after_marker.trim_start_matches('\n');

        let tab_block = Self::extract_tab_block(after_marker);
        let after_tab = &after_marker[tab_block.len()..];

        let workspace_vars = repos.first();

        let before = if let Some(vars) = workspace_vars {
            Self::replace_vars(before_marker, vars)
        } else {
            before_marker.to_string()
        };

        let after = if let Some(vars) = workspace_vars {
            Self::replace_vars(after_tab, vars)
        } else {
            after_tab.to_string()
        };

        let mut expanded = String::new();
        for (i, vars) in repos.iter().enumerate() {
            if i > 0 {
                expanded.push('\n');
            }
            expanded.push_str(&Self::replace_vars(tab_block, vars));
        }

        format!("{}\n\n{}{}", before.trim_end_matches('\n'), expanded, after)
    }

    fn extract_tab_block(s: &str) -> &str {
        let mut depth = 0;
        let mut started = false;
        let mut end = 0;

        for (i, ch) in s.char_indices() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => {
                    depth -= 1;
                    if started && depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        &s[..end]
    }

    pub fn default_layout() -> &'static str {
        r#"// 自动生成，修改无效，仅作参考和调试用途
layout {
    tab name="overview" {
        pane size=1 borderless=true {
            plugin location="tab-bar"
        }
        pane split_direction="vertical" {
            pane command="zootree" {
                args "list" "--status" "in_progress"
            }
            pane cwd="$workspace_dir"
        }
        pane size=1 borderless=true {
            plugin location="status-bar"
        }
    }

    // @repeat-per-repo
    tab name="$repo_name" {
        pane size=1 borderless=true {
            plugin location="tab-bar"
        }
        pane split_direction="vertical" {
            pane size="60%" command="lazygit" {
                args "-p" "$worktree_path" "-ucf" "$lazygit_config"
            }
            pane {
                pane size="30%" cwd="$worktree_path"
                pane size="70%" cwd="$worktree_path"
            }
        }
        pane size=1 borderless=true {
            plugin location="status-bar"
        }
    }
}"#
    }
}
