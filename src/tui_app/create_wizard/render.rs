use crate::cli::create_flow::{AfterCreateMode, CreateDraftError, CreateWizardLayout};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::{
    repo_list_label, review_repo_label, CreateWizardApp, CreateWizardPage, WizardTextField,
};

impl CreateWizardApp {
    fn scroll_visible_height(area: Rect) -> u16 {
        area.height.saturating_sub(2).max(1)
    }

    fn scroll_visible_width(area: Rect) -> u16 {
        area.width.saturating_sub(2).max(1)
    }

    fn max_scroll_for_lines(line_count: usize, visible_height: u16) -> usize {
        line_count
            .saturating_sub(visible_height as usize)
            .min(u16::MAX as usize)
    }

    fn wrapped_line_count(lines: &[Line<'_>], visible_width: u16) -> usize {
        let width = visible_width.max(1) as usize;
        lines
            .iter()
            .map(|line| {
                let text = line.to_string();
                textwrap::wrap(&text, width).len().max(1)
            })
            .sum()
    }

    fn clamp_scroll_to_wrapped_lines(
        scroll: &mut usize,
        lines: &[Line<'_>],
        visible_height: u16,
        visible_width: u16,
    ) {
        let line_count = Self::wrapped_line_count(lines, visible_width);
        *scroll = (*scroll).min(Self::max_scroll_for_lines(line_count, visible_height));
    }

    fn scroll_page_amount(visible_height: u16) -> usize {
        visible_height.saturating_sub(1).max(1) as usize
    }

    pub(super) fn review_page_scroll_amount(&self) -> usize {
        Self::scroll_page_amount(self.last_review_height)
    }

    pub(super) fn draft_page_scroll_amount(&self) -> usize {
        Self::scroll_page_amount(self.last_draft_height)
    }

    pub(super) fn scroll_review_down(&mut self, amount: usize) {
        self.review_scroll = self.review_scroll.saturating_add(amount);
    }

    pub(super) fn scroll_review_up(&mut self, amount: usize) {
        self.review_scroll = self.review_scroll.saturating_sub(amount);
    }

    pub(super) fn scroll_draft_down(&mut self, amount: usize) {
        self.draft_scroll = self.draft_scroll.saturating_add(amount);
    }

    pub(super) fn scroll_draft_up(&mut self, amount: usize) {
        self.draft_scroll = self.draft_scroll.saturating_sub(amount);
    }

    pub(super) fn render_frame(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        match CreateWizardLayout::for_width(area.width) {
            CreateWizardLayout::TooNarrow => self.render_too_narrow(frame, area),
            CreateWizardLayout::TwoColumn => self.render_two_column(frame, area),
            CreateWizardLayout::SingleColumn => self.render_single_column(frame, area),
        }
    }

    fn render_too_narrow(&self, frame: &mut ratatui::Frame, area: Rect) {
        let message = Paragraph::new(vec![
            Line::from(Span::styled(
                "Create wizard",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("resize to at least 50 columns"),
        ])
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });
        frame.render_widget(message, area);
    }

    fn render_two_column(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(chunks[0]);

        self.render_step(frame, columns[0]);
        self.render_summary(frame, columns[1], false);
        frame.render_widget(self.help_paragraph(), chunks[1]);
    }

    fn render_single_column(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        self.render_summary(frame, chunks[0], true);
        self.render_step(frame, chunks[1]);
        frame.render_widget(self.help_paragraph(), chunks[2]);
    }

    fn page_title(&self) -> String {
        self.page().title()
    }

    fn after_create_label(&self) -> String {
        match &self.draft.after_create {
            AfterCreateMode::CreateOnly => "create only".into(),
            AfterCreateMode::Start => "start workspace".into(),
            AfterCreateMode::StartAndRunAgent { run_agent } => match run_agent {
                Some(agent) => format!("start and run agent: {agent}"),
                None => self
                    .global
                    .agent_cli
                    .as_ref()
                    .map(|agent| format!("start and run agent: {agent}"))
                    .unwrap_or_else(|| "start and run agent".into()),
            },
        }
    }

    fn summary_lines(&self, compact: bool) -> Vec<Line<'static>> {
        let selected_count = self.selected_repo_count();
        if compact {
            let mut lines = vec![
                Line::from(Span::styled(
                    "Draft",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(format!(
                    "title: {} | name: {} | repos: {}",
                    self.draft.title, self.draft.name, selected_count
                )),
            ];
            lines.extend(self.description_summary_lines("desc"));
            lines.extend([
                Line::from(format!("branch: {}", self.draft.branch)),
                Line::from(format!("dir: {}", self.draft.workspace_dir)),
                Line::from(format!("after: {}", self.after_create_label())),
            ]);
            lines
        } else {
            let mut lines = vec![
                Line::from(Span::styled(
                    "Draft",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(format!("title: {}", self.draft.title)),
            ];
            lines.extend(self.description_summary_lines("description"));
            lines.extend([
                Line::from(format!("name: {}", self.draft.name)),
                Line::from(format!("branch: {}", self.draft.branch)),
                Line::from(format!("workspace_dir: {}", self.draft.workspace_dir)),
                Line::from(format!("repos: {}", selected_count)),
                Line::from(format!("after-create: {}", self.after_create_label())),
            ]);
            lines
        }
    }

    fn description_summary_lines(&self, label: &str) -> Vec<Line<'static>> {
        self.draft
            .description
            .split('\n')
            .enumerate()
            .map(|(index, line)| {
                if index == 0 {
                    Line::from(format!("{label}: {line}"))
                } else {
                    Line::from(format!("  {line}"))
                }
            })
            .collect()
    }

    fn render_summary(&mut self, frame: &mut ratatui::Frame, area: Rect, compact: bool) {
        let lines = self.summary_lines(compact);
        self.last_draft_height = Self::scroll_visible_height(area);
        let visible_width = Self::scroll_visible_width(area);
        Self::clamp_scroll_to_wrapped_lines(
            &mut self.draft_scroll,
            &lines,
            self.last_draft_height,
            visible_width,
        );
        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Draft"))
            .wrap(Wrap { trim: false })
            .scroll((self.draft_scroll as u16, 0));
        frame.render_widget(paragraph, area);
    }

    fn render_step(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        if let Some(field) = &self.text_field {
            self.render_text_page(frame, area, field);
        } else if self.page() == &CreateWizardPage::Repos {
            frame.render_widget(self.repos_page_paragraph(area), area);
        } else if self.page() == &CreateWizardPage::Review {
            self.render_review_page(frame, area);
        } else {
            frame.render_widget(self.page_paragraph(), area);
        }
    }

    fn render_text_page(&self, frame: &mut ratatui::Frame, area: Rect, field: &WizardTextField) {
        let mut textarea = field.textarea.clone();
        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(self.page_title()),
        );

        let mut detail_lines: Vec<Line<'static>> = self
            .errors
            .iter()
            .map(|err| {
                Line::from(Span::styled(
                    format!("error: {}", Self::error_message(err)),
                    Style::default().fg(Color::Red),
                ))
            })
            .collect();
        detail_lines.extend(self.page_context_lines());
        if detail_lines.is_empty() {
            frame.render_widget(&textarea, area);
            return;
        }

        let detail_height = (detail_lines.len() as u16).min(area.height.saturating_sub(1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(detail_height)])
            .split(area);
        frame.render_widget(&textarea, chunks[0]);
        frame.render_widget(
            Paragraph::new(detail_lines).wrap(Wrap { trim: false }),
            chunks[1],
        );
    }

    fn page_paragraph(&self) -> Paragraph<'static> {
        let title = self.page_title();
        let mut lines = vec![Line::from(Span::styled(
            title.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(""));
        lines.extend(self.page_content_lines());
        lines.extend(self.error_lines());

        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
    }

    fn render_review_page(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let title = self.page_title();
        let mut lines = vec![Line::from(Span::styled(
            title.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(""));
        lines.extend(self.page_content_lines());
        lines.extend(self.error_lines());

        self.last_review_height = Self::scroll_visible_height(area);
        let visible_width = Self::scroll_visible_width(area);
        Self::clamp_scroll_to_wrapped_lines(
            &mut self.review_scroll,
            &lines,
            self.last_review_height,
            visible_width,
        );

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .scroll((self.review_scroll as u16, 0));
        frame.render_widget(paragraph, area);
    }

    fn repos_page_paragraph(&self, area: Rect) -> Paragraph<'static> {
        let title = self.page_title();
        let inner_height = area.height.saturating_sub(2) as usize;
        let error_lines = self.error_lines();
        let mut lines = vec![Line::from(format!(
            "filter{}: {}",
            if self.repo_filter_active {
                " [active]"
            } else {
                ""
            },
            self.repo_filter_text()
        ))];
        if inner_height > 2 {
            lines.push(Line::from(""));
        }

        let repo_capacity = inner_height.saturating_sub(lines.len() + error_lines.len());
        let visible_indices = self.visible_repo_indices();
        if visible_indices.is_empty() {
            if repo_capacity > 0 {
                lines.push(Line::from("No repos match filter."));
            }
        } else {
            let window_start =
                Self::repo_window_start(visible_indices.len(), self.repo_cursor, repo_capacity);
            for (visible_idx, raw_idx) in visible_indices
                .into_iter()
                .enumerate()
                .skip(window_start)
                .take(repo_capacity)
            {
                let repo = &self.draft.repos[raw_idx];
                lines.push(Line::from(repo_list_label(
                    repo,
                    repo.selected,
                    visible_idx == self.repo_cursor,
                )));
            }
        }
        lines.extend(error_lines);

        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
    }

    fn page_content_lines(&self) -> Vec<Line<'static>> {
        match self.page() {
            CreateWizardPage::Repos => self
                .visible_repo_indices()
                .into_iter()
                .enumerate()
                .map(|(visible_idx, raw_idx)| {
                    let repo = &self.draft.repos[raw_idx];
                    Line::from(repo_list_label(
                        repo,
                        repo.selected,
                        visible_idx == self.repo_cursor,
                    ))
                })
                .fold(
                    vec![
                        Line::from(format!(
                            "filter{}: {}",
                            if self.repo_filter_active {
                                " [active]"
                            } else {
                                ""
                            },
                            self.repo_filter_text()
                        )),
                        Line::from(""),
                    ],
                    |mut lines, line| {
                        lines.push(line);
                        lines
                    },
                )
                .into_iter()
                .chain(
                    (self.visible_repo_indices().is_empty())
                        .then(|| Line::from("No repos match filter.")),
                )
                .collect(),
            CreateWizardPage::AfterCreate => {
                let options = [
                    (0, "Create only"),
                    (1, "Start workspace"),
                    (2, "Start and run agent"),
                ];
                let mut lines: Vec<Line<'static>> = options
                    .into_iter()
                    .map(|(idx, label)| {
                        let cursor = if idx == self.after_create_cursor {
                            ">"
                        } else {
                            " "
                        };
                        Line::from(format!("{cursor} {label}"))
                    })
                    .collect();
                lines.push(Line::from(format!(
                    "selected: {}",
                    self.after_create_label()
                )));
                lines
            }
            CreateWizardPage::Review => {
                let mut lines = self.summary_lines(false);
                lines.push(Line::from(""));
                lines.push(Line::from("Selected repos:"));
                for repo in self.draft.selected_repos() {
                    lines.push(Line::from(review_repo_label(repo)));
                }
                lines
            }
            _ => Vec::new(),
        }
    }

    fn page_context_lines(&self) -> Vec<Line<'static>> {
        match self.page() {
            CreateWizardPage::WorkspaceName => vec![
                Line::from(format!("derived branch: {}", self.draft.branch)),
                Line::from(format!("workspace_dir: {}", self.draft.workspace_dir)),
            ],
            CreateWizardPage::WorkspaceBranch => {
                vec![Line::from(format!(
                    "workspace_dir: {}",
                    self.draft.workspace_dir
                ))]
            }
            CreateWizardPage::TargetBranch { repo_name } => {
                vec![Line::from(format!("repo: {repo_name}"))]
            }
            CreateWizardPage::RunAgent => vec![Line::from(
                self.global
                    .agent_cli
                    .as_ref()
                    .map(|agent| format!("default agent: {agent}"))
                    .unwrap_or_else(|| "default agent: not configured".into()),
            )],
            _ => Vec::new(),
        }
    }

    fn error_lines(&self) -> Vec<Line<'static>> {
        if self.errors.is_empty() {
            return Vec::new();
        }
        let mut lines = vec![Line::from("")];
        lines.extend(self.errors.iter().map(|err| {
            Line::from(Span::styled(
                format!("error: {}", Self::error_message(err)),
                Style::default().fg(Color::Red),
            ))
        }));
        lines
    }

    fn error_message(err: &CreateDraftError) -> String {
        match err {
            CreateDraftError::TitleRequired => "title is required".into(),
            CreateDraftError::TitleSingleLineRequired => "title must be a single line".into(),
            CreateDraftError::WorkspaceNameRequired => "workspace name is required".into(),
            CreateDraftError::WorkspaceNameInvalid(name) => {
                format!("workspace name must use only ASCII letters, numbers, '-' and '_': {name}")
            }
            CreateDraftError::WorkspaceNameSingleLineRequired => {
                "workspace name must be a single line".into()
            }
            CreateDraftError::WorkspaceBranchRequired => "workspace branch is required".into(),
            CreateDraftError::WorkspaceBranchSingleLineRequired => {
                "workspace branch must be a single line".into()
            }
            CreateDraftError::WorkspaceNameExists(name) => {
                format!("workspace name already exists: {name}")
            }
            CreateDraftError::RepoRequired => "select at least one repo".into(),
            CreateDraftError::TargetBranchRequired(repo) => {
                format!("target branch is required for {repo}")
            }
            CreateDraftError::TargetBranchSingleLineRequired(repo) => {
                format!("target branch for {repo} must be a single line")
            }
            CreateDraftError::DefaultAgentMissing => {
                "configure a default agent or choose a different after-create action".into()
            }
            CreateDraftError::RunAgentSingleLineRequired => {
                "run-agent must be a single line".into()
            }
        }
    }

    fn help_paragraph(&self) -> Paragraph<'static> {
        let help = match self.page() {
            CreateWizardPage::Description => {
                "enter next · esc back · type description · alt/shift+enter newline · ctrl+a/e/u edit · ctrl+c abort"
            }
            CreateWizardPage::Title
            | CreateWizardPage::WorkspaceName
            | CreateWizardPage::WorkspaceBranch => {
                "enter next · esc back/cancel · type field value · ctrl+a/e/u edit · ctrl+c abort"
            }
            CreateWizardPage::TargetBranch { .. } => {
                "enter next · esc back · type branch name · ctrl+a/e/u edit · ctrl+c abort"
            }
            CreateWizardPage::Repos => {
                "enter next · esc back · up/down/j/k move · tab filter · space toggle repo · ctrl+c abort"
            }
            CreateWizardPage::AfterCreate => {
                "enter next · esc back · up/down/j/k move · ctrl+c abort"
            }
            CreateWizardPage::RunAgent => {
                "enter next · esc back · type agent command · ctrl+a/e/u edit · ctrl+c abort"
            }
            CreateWizardPage::Review => {
                "enter submit · esc back · review: up/down/j/k/pg/home/end · draft: alt+same · ctrl+c abort"
            }
        };
        Paragraph::new(Span::styled(help, Style::default().fg(Color::DarkGray)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_scroll_accounts_for_wrapped_lines() {
        let long_text = format!(
            "description: {} bottom-marker",
            (0..20)
                .map(|idx| format!("wrapped-token-{idx:02}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let lines = [Line::from(long_text)];

        assert!(CreateWizardApp::wrapped_line_count(&lines, 20) > 1);

        let mut scroll = usize::MAX;
        CreateWizardApp::clamp_scroll_to_wrapped_lines(&mut scroll, &lines, 3, 20);

        assert!(scroll > 0);
        assert!(scroll < usize::MAX);
    }

    #[test]
    fn clamp_scroll_keeps_empty_line_at_top_when_visible() {
        let lines = [Line::from("")];
        let mut scroll = usize::MAX;

        CreateWizardApp::clamp_scroll_to_wrapped_lines(&mut scroll, &lines, 1, 20);

        assert_eq!(scroll, 0);
    }
}
