//! `InfoApp`: detailed single-workspace view.

use std::time::Duration;

use chrono::{DateTime, Local};

use crate::config::workspace::{WorkspaceConfig, WorkspaceStatus};
use crate::config::ConfigManager;

pub struct InfoApp {
    pub(crate) name: String,
    pub(crate) config_mgr: ConfigManager,
    pub(crate) state: Option<InfoState>,
    pub(crate) watch: bool,
    pub(crate) interval: Duration,
    pub(crate) quit: bool,
    pub(crate) last_error: Option<String>,
}

pub(crate) struct InfoState {
    pub status: WorkspaceStatus,
    pub workspace: WorkspaceConfig,
    pub loaded_at: DateTime<Local>,
}

impl InfoApp {
    pub fn new(name: String, config_mgr: ConfigManager, watch: bool, interval: Duration) -> Self {
        let mut app = Self {
            name,
            config_mgr,
            state: None,
            watch,
            interval,
            quit: false,
            last_error: None,
        };
        app.reload();
        app
    }

    pub(crate) fn reload(&mut self) {
        match self.config_mgr.load_workspace(&self.name) {
            Ok((status, workspace)) => {
                self.state = Some(InfoState {
                    status,
                    workspace,
                    loaded_at: Local::now(),
                });
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("{:#}", e));
            }
        }
    }
}

/// Parse an RFC3339 timestamp and re-format it in the local zone as
/// `YYYY-MM-DD HH:MM`. On parse failure, returns the original string.
pub fn format_rfc3339_to_minute(s: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| s.to_string())
}

pub(crate) fn format_time_of_day(dt: &DateTime<Local>) -> String {
    dt.format("%H:%M:%S").to_string()
}

/// Return up to the last `n` elements of the slice, preserving order.
pub fn last_n<T>(items: &[T], n: usize) -> &[T] {
    if items.len() <= n {
        items
    } else {
        &items[items.len() - n..]
    }
}

pub(crate) fn status_label(s: &WorkspaceStatus) -> &'static str {
    match s {
        WorkspaceStatus::Pending => "pending",
        WorkspaceStatus::InProgress => "in_progress",
        WorkspaceStatus::Done => "done",
        WorkspaceStatus::Canceled => "canceled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::ZellijConfig;

    fn sample_workspace(name: &str) -> WorkspaceConfig {
        WorkspaceConfig {
            title: "Demo title".into(),
            name: name.into(),
            description: "line one\nline two".into(),
            branch: format!("zootree/{}", name),
            workspace_dir: format!("/tmp/{}", name),
            created_at: "2026-05-10T14:22:00+08:00".into(),
            zellij: ZellijConfig::default(),
            repos: vec![],
            events: vec![],
        }
    }

    #[test]
    fn format_rfc3339_to_minute_parses_valid() {
        let s = "2026-05-10T14:22:00+08:00";
        // Exact output is timezone-dependent, so just check shape.
        let out = format_rfc3339_to_minute(s);
        assert_eq!(out.len(), 16);
        assert_eq!(&out[4..5], "-");
        assert_eq!(&out[10..11], " ");
        assert_eq!(&out[13..14], ":");
    }

    #[test]
    fn format_rfc3339_to_minute_falls_back_on_invalid() {
        assert_eq!(format_rfc3339_to_minute("not-a-date"), "not-a-date");
    }

    #[test]
    fn last_n_returns_all_when_shorter() {
        let v = vec![1, 2, 3];
        assert_eq!(last_n(&v, 5), &[1, 2, 3]);
    }

    #[test]
    fn last_n_returns_tail_when_longer() {
        let v = vec![1, 2, 3, 4, 5];
        assert_eq!(last_n(&v, 3), &[3, 4, 5]);
    }

    #[test]
    fn last_n_handles_zero() {
        let v = vec![1, 2, 3];
        assert_eq!(last_n(&v, 0), &[] as &[i32]);
    }

    #[test]
    fn status_label_covers_all_variants() {
        assert_eq!(status_label(&WorkspaceStatus::Pending), "pending");
        assert_eq!(status_label(&WorkspaceStatus::InProgress), "in_progress");
        assert_eq!(status_label(&WorkspaceStatus::Done), "done");
        assert_eq!(status_label(&WorkspaceStatus::Canceled), "canceled");
    }

    #[test]
    fn reload_populates_state_for_existing_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace("demo");
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
            .unwrap();

        let mgr_for_app = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let app = InfoApp::new("demo".into(), mgr_for_app, false, Duration::from_secs(5));

        assert!(app.last_error.is_none());
        let state = app.state.as_ref().expect("state populated");
        assert!(matches!(state.status, WorkspaceStatus::InProgress));
        assert_eq!(state.workspace.name, "demo");
    }

    #[test]
    fn reload_records_error_for_missing_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();

        let app = InfoApp::new("ghost".into(), mgr, false, Duration::from_secs(5));
        assert!(app.state.is_none());
        assert!(app.last_error.is_some());
        assert!(app.last_error.as_deref().unwrap().contains("ghost"));
    }
}
