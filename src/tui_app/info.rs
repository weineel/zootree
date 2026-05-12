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

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};
use ratatui::Frame;

impl crate::tui_app::App for InfoApp {
    fn on_event(&mut self, event: crate::tui_app::Event) -> anyhow::Result<()> {
        use crate::tui_app::Event as E;
        use crossterm::event::{KeyCode, KeyModifiers};

        match event {
            E::Key(k) => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
                    KeyCode::Char('c') if ctrl => self.quit = true,
                    KeyCode::Char('r') => self.reload(),
                    _ => {}
                }
            }
            E::Tick => self.reload(),
            E::Resize(_, _) => {}
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::vertical([
            Constraint::Length(1), // title bar
            Constraint::Min(3),    // body
            Constraint::Length(1), // status line
        ])
        .split(area);

        self.render_title(frame, chunks[0]);
        self.render_body(frame, chunks[1]);
        self.render_status_line(frame, chunks[2]);
    }

    fn should_quit(&self) -> bool {
        self.quit
    }

    fn tick_interval(&self) -> Option<Duration> {
        if self.watch {
            Some(self.interval)
        } else {
            None
        }
    }
}

impl InfoApp {
    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let (title_text, color) = match &self.state {
            Some(s) => (
                format!(
                    "zootree info — {}  [{}]",
                    self.name,
                    status_label(&s.status)
                ),
                status_color(&s.status),
            ),
            None => (
                format!("zootree info — {}  [?]", self.name),
                Color::DarkGray,
            ),
        };
        let para = Paragraph::new(Span::styled(title_text, Style::default().fg(color)));
        frame.render_widget(para, area);
    }

    fn render_body(&self, frame: &mut Frame, area: Rect) {
        let Some(state) = &self.state else {
            let msg = self
                .last_error
                .clone()
                .unwrap_or_else(|| "loading...".into());
            let para = Paragraph::new(msg).block(Block::default().borders(Borders::ALL));
            frame.render_widget(para, area);
            return;
        };

        let ws = &state.workspace;

        // Compute meta block height: 4 fixed lines (Title/Branch/Dir/Created),
        // plus description block if non-empty (blank line + "Description:" + N lines).
        let desc_height = if ws.description.is_empty() {
            0
        } else {
            2 + ws.description.lines().count() as u16
        };
        let meta_height = 4 + desc_height;

        // Repos block: top border + header + rows (or 1 "(none)" row).
        let repos_rows = ws.repos.len().max(1) as u16;
        let repos_height = 2 + repos_rows;

        let chunks = Layout::vertical([
            Constraint::Length(meta_height),
            Constraint::Length(repos_height),
            Constraint::Min(1),
        ])
        .split(area);

        // Meta
        let mut lines: Vec<Line> = Vec::new();
        let created_str = format_rfc3339_to_minute(&ws.created_at);
        lines.push(meta_line("Title:", &ws.title));
        lines.push(meta_line("Branch:", &ws.branch));
        lines.push(meta_line("Dir:", &ws.workspace_dir));
        lines.push(meta_line("Created:", &created_str));
        if !ws.description.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("  Description:"));
            for l in ws.description.lines() {
                lines.push(Line::from(format!("    {}", l)));
            }
        }
        frame.render_widget(Paragraph::new(lines), chunks[0]);

        // Repos
        let rows: Vec<Row> = if ws.repos.is_empty() {
            vec![Row::new(vec![
                "(none)".to_string(),
                "".to_string(),
                "".to_string(),
            ])]
        } else {
            ws.repos
                .iter()
                .map(|r| {
                    let target = r.target_branch.as_deref().unwrap_or("*");
                    let worktree = format!("{}/{}", ws.workspace_dir, r.name);
                    Row::new(vec![r.name.clone(), target.to_string(), worktree])
                })
                .collect()
        };
        let table = Table::new(
            rows,
            [
                Constraint::Length(15),
                Constraint::Length(15),
                Constraint::Min(20),
            ],
        )
        .header(
            Row::new(vec!["NAME", "TARGET", "WORKTREE"])
                .style(Style::default().fg(Color::DarkGray)),
        )
        .block(Block::default().borders(Borders::TOP).title(" Repos "));
        frame.render_widget(table, chunks[1]);

        // Events
        let recent = last_n(&ws.events, 5);
        let items: Vec<ListItem> = recent
            .iter()
            .map(|e| {
                let ts = format_rfc3339_to_minute(&e.timestamp);
                let mut text = format!("{}  {}", ts, e.action);
                if let Some(d) = &e.detail {
                    text.push_str(&format!("  ({})", d));
                }
                ListItem::new(text)
            })
            .collect();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::TOP)
                .title(" Recent events "),
        );
        frame.render_widget(list, chunks[2]);
    }

    fn render_status_line(&self, frame: &mut Frame, area: Rect) {
        let left = "[q] quit   [r] reload".to_string();
        let right = if let Some(state) = &self.state {
            let mode = if self.watch {
                format!("watching ({}s)", self.interval.as_secs())
            } else {
                "once".to_string()
            };
            format!(
                "{}   updated {}",
                mode,
                format_time_of_day(&state.loaded_at)
            )
        } else {
            "loading".to_string()
        };

        let width = area.width as usize;
        let combined = if left.len() + right.len() + 2 <= width {
            let pad = width - left.len() - right.len();
            format!("{}{}{}", left, " ".repeat(pad), right)
        } else {
            format!("{}  {}", left, right)
        };

        frame.render_widget(
            Paragraph::new(combined).style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }
}

fn meta_line<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {:<10}", label),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(value.to_string()),
    ])
}

fn status_color(s: &WorkspaceStatus) -> Color {
    match s {
        WorkspaceStatus::Pending => Color::DarkGray,
        WorkspaceStatus::InProgress => Color::Green,
        WorkspaceStatus::Done => Color::Blue,
        WorkspaceStatus::Canceled => Color::Red,
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

    fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn render_to_string(app: &mut InfoApp, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| <InfoApp as crate::tui_app::App>::render(app, f))
            .unwrap();
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn render_shows_name_status_and_title() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace("demo");
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
            .unwrap();

        let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));

        let out = render_to_string(&mut app, 80, 20);
        assert!(out.contains("demo"), "missing name:\n{}", out);
        assert!(out.contains("in_progress"), "missing status:\n{}", out);
        assert!(out.contains("Demo title"), "missing title:\n{}", out);
        assert!(out.contains("zootree/demo"), "missing branch:\n{}", out);
    }

    #[test]
    fn render_shows_last_error_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let mut app = InfoApp::new("ghost".into(), mgr, false, Duration::from_secs(5));

        let out = render_to_string(&mut app, 80, 10);
        assert!(out.contains("ghost"), "error should mention name:\n{}", out);
    }

    #[test]
    fn render_shows_repos_row() {
        use crate::config::workspace::RepoEntry;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let mut ws = sample_workspace("demo");
        ws.repos = vec![RepoEntry {
            name: "frontend".into(),
            target_branch: Some("main".into()),
        }];
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
            .unwrap();

        let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let mut app = InfoApp::new("demo".into(), mgr2, false, Duration::from_secs(5));
        let out = render_to_string(&mut app, 100, 20);
        assert!(out.contains("frontend"), "missing repo name:\n{}", out);
        assert!(out.contains("main"), "missing target branch:\n{}", out);
    }

    fn make_in_progress_app(tmp: &tempfile::TempDir, name: &str) -> InfoApp {
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace(name);
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
            .unwrap();
        let mgr2 = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        InfoApp::new(name.into(), mgr2, true, Duration::from_secs(5))
    }

    #[test]
    fn key_q_sets_quit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let ev = crate::tui_app::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('q'),
            crossterm::event::KeyModifiers::NONE,
        ));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, ev).unwrap();
        assert!(<InfoApp as crate::tui_app::App>::should_quit(&app));
    }

    #[test]
    fn key_esc_sets_quit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let ev = crate::tui_app::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, ev).unwrap();
        assert!(<InfoApp as crate::tui_app::App>::should_quit(&app));
    }

    #[test]
    fn key_ctrl_c_sets_quit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let ev = crate::tui_app::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('c'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, ev).unwrap();
        assert!(<InfoApp as crate::tui_app::App>::should_quit(&app));
    }

    #[test]
    fn key_r_triggers_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let first_loaded = app.state.as_ref().unwrap().loaded_at;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ev = crate::tui_app::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::NONE,
        ));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, ev).unwrap();
        let second_loaded = app.state.as_ref().unwrap().loaded_at;
        assert!(second_loaded > first_loaded);
    }

    #[test]
    fn tick_triggers_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        let first_loaded = app.state.as_ref().unwrap().loaded_at;
        std::thread::sleep(std::time::Duration::from_millis(10));
        <InfoApp as crate::tui_app::App>::on_event(&mut app, crate::tui_app::Event::Tick).unwrap();
        let second_loaded = app.state.as_ref().unwrap().loaded_at;
        assert!(second_loaded > first_loaded);
    }

    #[test]
    fn resize_does_not_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = make_in_progress_app(&tmp, "demo");
        <InfoApp as crate::tui_app::App>::on_event(
            &mut app,
            crate::tui_app::Event::Resize(120, 40),
        )
        .unwrap();
        assert!(!<InfoApp as crate::tui_app::App>::should_quit(&app));
    }

    #[test]
    fn tick_interval_reflects_watch_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let ws = sample_workspace("demo");
        mgr.save_workspace(&WorkspaceStatus::InProgress, &ws)
            .unwrap();

        let mgr_watch = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let watching = InfoApp::new("demo".into(), mgr_watch, true, Duration::from_secs(7));
        assert_eq!(
            <InfoApp as crate::tui_app::App>::tick_interval(&watching),
            Some(Duration::from_secs(7))
        );

        let mgr_once = ConfigManager::with_base_dir(tmp.path().to_path_buf());
        let once = InfoApp::new("demo".into(), mgr_once, false, Duration::from_secs(5));
        assert_eq!(<InfoApp as crate::tui_app::App>::tick_interval(&once), None);
    }
}
