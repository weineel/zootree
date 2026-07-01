use std::cell::RefCell;
use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

use crate::cli::create_flow::{
    AfterCreateMode, CreateDraft, CreateDraftError, CreateWizardLayout, CreateWizardOutput,
};
use crate::config::global::GlobalConfig;
use crate::tui_app::{run_app, App, CancelledByUser, Event};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui_textarea::{CursorMove, TextArea, WrapMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateStep {
    Info,
    Repos,
    Branches,
    AfterCreate,
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateWizardPage {
    Title,
    Description,
    WorkspaceName,
    WorkspaceBranch,
    Repos,
    TargetBranch { repo_name: String },
    AfterCreate,
    RunAgent,
    Review,
}

impl CreateWizardPage {
    pub fn title(&self) -> String {
        match self {
            Self::Title => "Workspace: Title".into(),
            Self::Description => "Workspace: Description".into(),
            Self::WorkspaceName => "Workspace: Name".into(),
            Self::WorkspaceBranch => "Workspace: Branch".into(),
            Self::Repos => "Repos".into(),
            Self::TargetBranch { repo_name } => format!("Branches: {repo_name}"),
            Self::AfterCreate => "After create".into(),
            Self::RunAgent => "After create: Run agent".into(),
            Self::Review => "Review".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateWizardOutcome {
    Submit(CreateWizardOutput),
    Cancelled,
}

pub fn repo_list_label(
    repo: &crate::cli::create_flow::RepoDraftEntry,
    selected: bool,
    focused: bool,
) -> String {
    let cursor = if focused { ">" } else { " " };
    let selected = if selected { "[x]" } else { "[ ]" };
    format!("{cursor} {selected} {}", repo.display_name())
}

pub fn review_repo_label(repo: &crate::cli::create_flow::RepoDraftEntry) -> String {
    format!("- {} -> {}", repo.display_name(), repo.target_branch)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardTextKind {
    SingleLine,
    Multiline,
}

struct WizardTextField {
    textarea: TextArea<'static>,
}

impl WizardTextField {
    fn new(text: impl Into<String>, _kind: WizardTextKind) -> Self {
        let text = text.into();
        let mut textarea = if text.is_empty() {
            TextArea::default()
        } else {
            TextArea::from(text.split('\n').map(str::to_string).collect::<Vec<_>>())
        };
        textarea.move_cursor(CursorMove::Bottom);
        textarea.move_cursor(CursorMove::End);
        textarea.set_cursor_line_style(Style::default());
        textarea.set_wrap_mode(WrapMode::WordOrGlyph);
        Self { textarea }
    }

    fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('u') if ctrl => {
                let _ = self.textarea.delete_line_by_head();
            }
            KeyCode::Enter if alt || shift => self.textarea.insert_newline(),
            _ => {
                let _ = self.textarea.input(key);
            }
        }
    }

    fn handle_paste(&mut self, text: &str) {
        self.textarea.insert_str(text);
    }
}

pub struct CreateWizardApp {
    draft: CreateDraft,
    global: GlobalConfig,
    existing_workspaces: Vec<String>,
    pages: Vec<CreateWizardPage>,
    page_index: usize,
    repo_cursor: usize,
    repo_filter: WizardTextField,
    repo_filter_active: bool,
    after_create_cursor: usize,
    run_agent_value: Option<String>,
    text_field: Option<WizardTextField>,
    errors: Vec<CreateDraftError>,
    outcome: Rc<RefCell<Option<CreateWizardOutcome>>>,
}

impl CreateWizardApp {
    pub fn new(draft: CreateDraft, global: GlobalConfig, existing_workspaces: Vec<String>) -> Self {
        Self::with_outcome_handle(
            draft,
            global,
            existing_workspaces,
            Rc::new(RefCell::new(None)),
        )
    }

    pub fn with_outcome_handle(
        draft: CreateDraft,
        global: GlobalConfig,
        existing_workspaces: Vec<String>,
        outcome: Rc<RefCell<Option<CreateWizardOutcome>>>,
    ) -> Self {
        let after_create_cursor = Self::after_create_cursor_for_mode(&draft.after_create);
        let run_agent_value = match &draft.after_create {
            AfterCreateMode::StartAndRunAgent { run_agent } => run_agent.clone(),
            _ => None,
        };
        let pages = Self::build_pages(&draft);
        let mut app = Self {
            draft,
            global,
            existing_workspaces,
            pages,
            page_index: 0,
            repo_cursor: 0,
            repo_filter: WizardTextField::new("", WizardTextKind::SingleLine),
            repo_filter_active: false,
            after_create_cursor,
            run_agent_value,
            text_field: None,
            errors: Vec::new(),
            outcome,
        };
        app.enter_page(0);
        app
    }

    pub fn step(&self) -> CreateStep {
        Self::step_for_page(self.page())
    }

    pub fn set_step(&mut self, step: CreateStep) {
        self.enter_page(self.first_page_index_for_step(step));
        self.clamp_active_cursor();
        if self.step() == CreateStep::AfterCreate {
            self.after_create_cursor = Self::after_create_cursor_for_mode(&self.draft.after_create);
        }
    }

    pub fn page(&self) -> &CreateWizardPage {
        &self.pages[self.page_index]
    }

    pub fn page_titles(&self) -> Vec<String> {
        self.pages.iter().map(CreateWizardPage::title).collect()
    }

    pub fn draft(&self) -> &CreateDraft {
        &self.draft
    }

    pub fn repo_cursor(&self) -> usize {
        self.repo_cursor
    }

    pub fn after_create_cursor(&self) -> usize {
        self.after_create_cursor
    }

    pub fn errors(&self) -> &[CreateDraftError] {
        &self.errors
    }

    pub fn outcome(&self) -> Option<CreateWizardOutcome> {
        self.outcome.borrow().clone()
    }

    fn build_pages(draft: &CreateDraft) -> Vec<CreateWizardPage> {
        let mut pages = vec![
            CreateWizardPage::Title,
            CreateWizardPage::Description,
            CreateWizardPage::WorkspaceName,
            CreateWizardPage::WorkspaceBranch,
            CreateWizardPage::Repos,
        ];
        pages.extend(draft.selected_repos().into_iter().map(|repo| {
            CreateWizardPage::TargetBranch {
                repo_name: repo.name.clone(),
            }
        }));
        pages.push(CreateWizardPage::AfterCreate);
        if matches!(draft.after_create, AfterCreateMode::StartAndRunAgent { .. }) {
            pages.push(CreateWizardPage::RunAgent);
        }
        pages.push(CreateWizardPage::Review);
        pages
    }

    fn page_text_kind(page: &CreateWizardPage) -> Option<WizardTextKind> {
        match page {
            CreateWizardPage::Description => Some(WizardTextKind::Multiline),
            CreateWizardPage::Title
            | CreateWizardPage::WorkspaceName
            | CreateWizardPage::WorkspaceBranch
            | CreateWizardPage::TargetBranch { .. }
            | CreateWizardPage::RunAgent => Some(WizardTextKind::SingleLine),
            _ => None,
        }
    }

    fn step_for_page(page: &CreateWizardPage) -> CreateStep {
        match page {
            CreateWizardPage::Title
            | CreateWizardPage::Description
            | CreateWizardPage::WorkspaceName
            | CreateWizardPage::WorkspaceBranch => CreateStep::Info,
            CreateWizardPage::Repos => CreateStep::Repos,
            CreateWizardPage::TargetBranch { .. } => CreateStep::Branches,
            CreateWizardPage::AfterCreate | CreateWizardPage::RunAgent => CreateStep::AfterCreate,
            CreateWizardPage::Review => CreateStep::Review,
        }
    }

    fn first_page_index_for_step(&self, step: CreateStep) -> usize {
        self.pages
            .iter()
            .position(|page| Self::step_for_page(page) == step)
            .unwrap_or(0)
    }

    fn enter_page(&mut self, page_index: usize) {
        self.page_index = page_index.min(self.pages.len().saturating_sub(1));
        self.errors.clear();
        self.sync_current_page_state(true);
    }

    fn sync_current_page_state(&mut self, _reset_editing: bool) {
        self.text_field = self
            .current_page_text()
            .map(|(text, kind)| WizardTextField::new(text, kind));
    }

    fn current_page_text(&self) -> Option<(String, WizardTextKind)> {
        let kind = Self::page_text_kind(self.page())?;
        match self.page() {
            CreateWizardPage::Title => Some((self.draft.title.clone(), kind)),
            CreateWizardPage::Description => Some((self.draft.description.clone(), kind)),
            CreateWizardPage::WorkspaceName => Some((self.draft.name.clone(), kind)),
            CreateWizardPage::WorkspaceBranch => Some((self.draft.branch.clone(), kind)),
            CreateWizardPage::TargetBranch { repo_name } => self
                .draft
                .repo(repo_name)
                .map(|repo| (repo.target_branch.clone(), kind)),
            CreateWizardPage::RunAgent => Some((
                self.run_agent_value
                    .clone()
                    .or_else(|| {
                        if let AfterCreateMode::StartAndRunAgent { run_agent } =
                            &self.draft.after_create
                        {
                            run_agent.clone()
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default(),
                kind,
            )),
            _ => None,
        }
    }

    fn clamp_page_index(&mut self) {
        if self.pages.is_empty() {
            self.page_index = 0;
        } else {
            self.page_index = self.page_index.min(self.pages.len() - 1);
        }
    }

    fn refresh_pages(&mut self) {
        let previous_page = self.pages.get(self.page_index).cloned();
        let previous_step = previous_page
            .as_ref()
            .map(Self::step_for_page)
            .unwrap_or_else(|| self.step());
        let previous_index = self.page_index;
        self.pages = Self::build_pages(&self.draft);
        let next_page_index = previous_page
            .as_ref()
            .and_then(|page| self.pages.iter().position(|candidate| candidate == page))
            .or_else(|| {
                self.pages
                    .iter()
                    .enumerate()
                    .filter(|(_, page)| Self::step_for_page(page) == previous_step)
                    .min_by_key(|(idx, _)| idx.abs_diff(previous_index))
                    .map(|(idx, _)| idx)
            })
            .unwrap_or_else(|| previous_index.min(self.pages.len().saturating_sub(1)));
        let page_changed = previous_page
            .as_ref()
            .is_none_or(|page| self.pages.get(next_page_index) != Some(page));
        self.page_index = next_page_index;
        self.sync_current_page_state(page_changed);
    }

    fn error_belongs_to_step(step: CreateStep, err: &CreateDraftError) -> bool {
        match step {
            CreateStep::Info => matches!(
                err,
                CreateDraftError::TitleRequired
                    | CreateDraftError::TitleSingleLineRequired
                    | CreateDraftError::WorkspaceNameRequired
                    | CreateDraftError::WorkspaceNameSingleLineRequired
                    | CreateDraftError::WorkspaceBranchRequired
                    | CreateDraftError::WorkspaceBranchSingleLineRequired
                    | CreateDraftError::WorkspaceNameExists(_)
            ),
            CreateStep::Repos => matches!(err, CreateDraftError::RepoRequired),
            CreateStep::Branches => matches!(
                err,
                CreateDraftError::TargetBranchRequired(_)
                    | CreateDraftError::TargetBranchSingleLineRequired(_)
            ),
            CreateStep::AfterCreate => matches!(
                err,
                CreateDraftError::DefaultAgentMissing
                    | CreateDraftError::RunAgentSingleLineRequired
            ),
            CreateStep::Review => true,
        }
    }

    fn validate_current_step(&mut self) -> bool {
        self.refresh_current_step_errors();
        self.errors.is_empty()
    }

    fn refresh_current_step_errors(&mut self) {
        let all_errors = self.draft.validate(&self.existing_workspaces, &self.global);
        let step = self.step();
        self.errors = all_errors
            .into_iter()
            .filter(|err| Self::error_belongs_to_step(step, err))
            .collect();
    }

    fn clean_single_line(field: &str, text: String) -> Result<String, CreateDraftError> {
        let cleaned = text.trim().to_string();
        if cleaned.is_empty() {
            return Err(match field {
                "title" => CreateDraftError::TitleRequired,
                "workspace_name" => CreateDraftError::WorkspaceNameRequired,
                "workspace_branch" => CreateDraftError::WorkspaceBranchRequired,
                _ => CreateDraftError::TargetBranchRequired(field.into()),
            });
        }
        if text.contains('\n') {
            return Err(match field {
                "title" => CreateDraftError::TitleSingleLineRequired,
                "workspace_name" => CreateDraftError::WorkspaceNameSingleLineRequired,
                "workspace_branch" => CreateDraftError::WorkspaceBranchSingleLineRequired,
                _ => CreateDraftError::TargetBranchSingleLineRequired(field.into()),
            });
        }
        Ok(cleaned)
    }

    fn clean_target_branch(repo_name: &str, text: String) -> Result<String, CreateDraftError> {
        let cleaned = text.trim().to_string();
        if cleaned.is_empty() {
            return Err(CreateDraftError::TargetBranchRequired(repo_name.into()));
        }
        if text.contains('\n') {
            return Err(CreateDraftError::TargetBranchSingleLineRequired(
                repo_name.into(),
            ));
        }
        Ok(cleaned)
    }

    fn commit_current_page(&mut self) -> bool {
        match self.page() {
            CreateWizardPage::Title => {
                let text = self
                    .text_field
                    .as_ref()
                    .map(WizardTextField::text)
                    .unwrap_or_default();
                match Self::clean_single_line("title", text) {
                    Ok(title) => self.draft.title = title,
                    Err(err) => {
                        self.errors = vec![err];
                        return false;
                    }
                }
            }
            CreateWizardPage::Description => {
                self.draft.description = self
                    .text_field
                    .as_ref()
                    .map(WizardTextField::text)
                    .unwrap_or_default();
            }
            CreateWizardPage::WorkspaceName => {
                let text = self
                    .text_field
                    .as_ref()
                    .map(WizardTextField::text)
                    .unwrap_or_default();
                match Self::clean_single_line("workspace_name", text) {
                    Ok(name) => {
                        self.draft.set_name(name, &self.global);
                        if self
                            .existing_workspaces
                            .iter()
                            .any(|existing| existing == &self.draft.name)
                        {
                            self.errors = vec![CreateDraftError::WorkspaceNameExists(
                                self.draft.name.clone(),
                            )];
                            return false;
                        }
                    }
                    Err(err) => {
                        self.errors = vec![err];
                        return false;
                    }
                }
            }
            CreateWizardPage::WorkspaceBranch => {
                let text = self
                    .text_field
                    .as_ref()
                    .map(WizardTextField::text)
                    .unwrap_or_default();
                match Self::clean_single_line("workspace_branch", text) {
                    Ok(branch) => self.draft.set_branch(branch),
                    Err(err) => {
                        self.errors = vec![err];
                        return false;
                    }
                }
            }
            CreateWizardPage::TargetBranch { repo_name } => {
                let repo_name = repo_name.clone();
                let text = self
                    .text_field
                    .as_ref()
                    .map(WizardTextField::text)
                    .unwrap_or_default();
                match Self::clean_target_branch(&repo_name, text) {
                    Ok(target_branch) => {
                        if let Some(repo) = self.repo_mut(&repo_name) {
                            repo.target_branch = target_branch;
                        }
                    }
                    Err(err) => {
                        self.errors = vec![err];
                        return false;
                    }
                }
            }
            CreateWizardPage::AfterCreate => {
                self.draft.after_create =
                    self.mode_for_after_create_cursor(self.after_create_cursor);
                self.refresh_pages();
            }
            CreateWizardPage::RunAgent => {
                let text = self
                    .text_field
                    .as_ref()
                    .map(WizardTextField::text)
                    .unwrap_or_default();
                if text.contains('\n') {
                    self.errors = vec![CreateDraftError::RunAgentSingleLineRequired];
                    return false;
                }
                let value = text.trim().to_string();
                let next_run_agent = if value.is_empty() { None } else { Some(value) };
                if next_run_agent.is_none() && self.global.agent_cli.is_none() {
                    self.errors = vec![CreateDraftError::DefaultAgentMissing];
                    return false;
                }
                self.run_agent_value = next_run_agent;
                self.draft.after_create = AfterCreateMode::StartAndRunAgent {
                    run_agent: self.run_agent_value.clone(),
                };
            }
            _ => return self.validate_current_step(),
        }
        true
    }

    fn commit_text_page_if_present(&mut self) -> bool {
        if self.text_field.is_some() {
            self.commit_current_page()
        } else {
            true
        }
    }

    fn should_commit_before_back(&self) -> bool {
        matches!(
            self.page(),
            CreateWizardPage::Title
                | CreateWizardPage::Description
                | CreateWizardPage::WorkspaceName
                | CreateWizardPage::WorkspaceBranch
        )
    }

    fn submit_or_advance(&mut self) {
        if !self.commit_current_page() {
            return;
        }
        if matches!(self.page(), CreateWizardPage::Review) {
            *self.outcome.borrow_mut() = Some(CreateWizardOutcome::Submit(CreateWizardOutput {
                draft: self.draft.clone(),
            }));
        } else {
            self.enter_page(self.page_index + 1);
        }
    }

    fn cancel_or_back(&mut self) {
        if self.page_index == 0 {
            *self.outcome.borrow_mut() = Some(CreateWizardOutcome::Cancelled);
        } else {
            if matches!(self.page(), CreateWizardPage::TargetBranch { .. }) {
                if self.commit_current_page() {
                    self.enter_page(self.page_index.saturating_sub(1));
                } else if self
                    .errors
                    .iter()
                    .any(|err| matches!(err, CreateDraftError::TargetBranchRequired(_)))
                {
                    self.enter_page(self.first_page_index_for_step(CreateStep::Repos));
                }
                return;
            }
            if matches!(self.page(), CreateWizardPage::RunAgent) {
                let _ = self.commit_current_page();
                self.enter_page(self.page_index.saturating_sub(1));
                return;
            }
            if self.should_commit_before_back() && !self.commit_current_page() {
                return;
            }
            self.enter_page(self.page_index.saturating_sub(1));
        }
    }

    fn toggle_current_repo(&mut self) {
        let visible = self.visible_repo_indices();
        if visible.is_empty() {
            return;
        }
        let idx = visible[self.repo_cursor.min(visible.len() - 1)];
        self.draft.repos[idx].selected = !self.draft.repos[idx].selected;
        self.clamp_active_cursor();
        self.refresh_current_step_errors();
        self.refresh_pages();
    }

    fn move_repo_cursor(&mut self, delta: isize) {
        let len = self.visible_repo_indices().len();
        if len == 0 {
            self.repo_cursor = 0;
            return;
        }
        let len = len as isize;
        self.repo_cursor = (self.repo_cursor as isize + delta).rem_euclid(len) as usize;
    }

    fn repo_filter_text(&self) -> String {
        self.repo_filter.text()
    }

    fn visible_repo_indices(&self) -> Vec<usize> {
        let filter = self.repo_filter_text().trim().to_lowercase();
        self.draft
            .repos
            .iter()
            .enumerate()
            .filter_map(|(idx, repo)| {
                (filter.is_empty() || repo.name.to_lowercase().contains(&filter)).then_some(idx)
            })
            .collect()
    }

    fn selected_repo_count(&self) -> usize {
        self.draft.repos.iter().filter(|repo| repo.selected).count()
    }

    fn move_info_page(&mut self, delta: isize) {
        if !self.commit_text_page_if_present() {
            return;
        }
        let info_pages: Vec<usize> = self
            .pages
            .iter()
            .enumerate()
            .filter_map(|(idx, page)| {
                (Self::step_for_page(page) == CreateStep::Info).then_some(idx)
            })
            .collect();
        if info_pages.is_empty() {
            return;
        }
        let current_pos = info_pages
            .iter()
            .position(|idx| *idx == self.page_index)
            .unwrap_or(0);
        let last_pos = info_pages.len() as isize - 1;
        let next_pos = (current_pos as isize + delta).clamp(0, last_pos) as usize;
        self.enter_page(info_pages[next_pos]);
    }

    fn after_create_cursor_for_mode(mode: &AfterCreateMode) -> usize {
        match mode {
            AfterCreateMode::CreateOnly => 0,
            AfterCreateMode::Start => 1,
            AfterCreateMode::StartAndRunAgent { .. } => 2,
        }
    }

    fn mode_for_after_create_cursor(&self, cursor: usize) -> AfterCreateMode {
        match cursor {
            0 => AfterCreateMode::CreateOnly,
            1 => AfterCreateMode::Start,
            _ => AfterCreateMode::StartAndRunAgent {
                run_agent: self.run_agent_value.clone(),
            },
        }
    }

    fn apply_after_create_cursor(&mut self) {
        if let AfterCreateMode::StartAndRunAgent {
            run_agent: Some(run_agent),
        } = &self.draft.after_create
        {
            self.run_agent_value = Some(run_agent.clone());
        }
        self.draft.after_create = self.mode_for_after_create_cursor(self.after_create_cursor);
        self.refresh_pages();
        self.errors.clear();
    }

    fn move_after_create_cursor(&mut self, delta: isize) {
        self.after_create_cursor =
            (self.after_create_cursor as isize + delta).rem_euclid(3) as usize;
        self.apply_after_create_cursor();
    }

    fn move_active_cursor(&mut self, delta: isize) {
        match self.step() {
            CreateStep::Info => self.move_info_page(delta),
            CreateStep::Repos => self.move_repo_cursor(delta),
            CreateStep::Branches => {}
            CreateStep::AfterCreate => self.move_after_create_cursor(delta),
            CreateStep::Review => {}
        }
    }

    fn clamp_active_cursor(&mut self) {
        let visible_repo_count = self.visible_repo_indices().len();
        if visible_repo_count == 0 {
            self.repo_cursor = 0;
        } else {
            self.repo_cursor = self.repo_cursor.min(visible_repo_count - 1);
        }
        self.after_create_cursor = self.after_create_cursor.min(2);
        self.clamp_page_index();
    }

    fn repo_mut(&mut self, name: &str) -> Option<&mut crate::cli::create_flow::RepoDraftEntry> {
        self.draft.repos.iter_mut().find(|repo| repo.name == name)
    }

    fn handle_text_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        if let Some(field) = &mut self.text_field {
            field.handle_key(key);
            self.errors.clear();
            return true;
        }
        false
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

    fn render_two_column(&self, frame: &mut ratatui::Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(chunks[0]);

        self.render_step(frame, columns[0]);
        frame.render_widget(self.summary_paragraph(false), columns[1]);
        frame.render_widget(self.help_paragraph(), chunks[1]);
    }

    fn render_single_column(&self, frame: &mut ratatui::Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        frame.render_widget(self.summary_paragraph(true), chunks[0]);
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

    fn summary_paragraph(&self, compact: bool) -> Paragraph<'static> {
        Paragraph::new(self.summary_lines(compact))
            .block(Block::default().borders(Borders::ALL).title("Draft"))
            .wrap(Wrap { trim: false })
    }

    fn render_step(&self, frame: &mut ratatui::Frame, area: Rect) {
        if let Some(field) = &self.text_field {
            self.render_text_page(frame, area, field);
        } else if self.page() == &CreateWizardPage::Repos {
            frame.render_widget(self.repos_page_paragraph(area), area);
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

    fn repo_window_start(visible_count: usize, cursor: usize, capacity: usize) -> usize {
        if capacity == 0 || visible_count <= capacity {
            0
        } else {
            cursor
                .saturating_sub(capacity - 1)
                .min(visible_count - capacity)
        }
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
                "enter submit · esc back · ctrl+c abort"
            }
        };
        Paragraph::new(Span::styled(help, Style::default().fg(Color::DarkGray)))
    }
}

impl App for CreateWizardApp {
    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        let key = match event {
            Event::Paste(text) => {
                if let Some(field) = &mut self.text_field {
                    field.handle_paste(&text);
                    self.errors.clear();
                    return Ok(());
                }
                if self.step() == CreateStep::Repos && self.repo_filter_active {
                    self.repo_filter.handle_paste(&text);
                    self.clamp_active_cursor();
                    self.errors.clear();
                    return Ok(());
                }
                return Ok(());
            }
            Event::Key(key) => key,
            Event::Resize(_, _) | Event::Tick => return Ok(()),
        };
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let step = self.step();
        match key.code {
            KeyCode::Char('c') if ctrl => {
                *self.outcome.borrow_mut() = Some(CreateWizardOutcome::Cancelled);
            }
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT)
                    && self.handle_text_key(key) => {}
            KeyCode::Enter => self.submit_or_advance(),
            KeyCode::Esc => self.cancel_or_back(),
            _ if self.text_field.is_some() && self.handle_text_key(key) => {}
            KeyCode::Tab if step == CreateStep::Info => self.move_info_page(1),
            KeyCode::BackTab if step == CreateStep::Info => self.move_info_page(-1),
            KeyCode::Tab if step == CreateStep::Repos => {
                self.repo_filter_active = !self.repo_filter_active;
            }
            KeyCode::Down if step == CreateStep::Info => self.move_info_page(1),
            KeyCode::Up if step == CreateStep::Info => self.move_info_page(-1),
            KeyCode::Char('n') if ctrl && step == CreateStep::Info => self.move_info_page(1),
            KeyCode::Char('p') if ctrl && step == CreateStep::Info => self.move_info_page(-1),
            KeyCode::Char(' ') if step == CreateStep::Repos => self.toggle_current_repo(),
            KeyCode::Char(_) if !ctrl && step == CreateStep::Repos && self.repo_filter_active => {
                self.repo_filter.handle_key(key);
                self.clamp_active_cursor();
                self.errors.clear();
            }
            KeyCode::Backspace if step == CreateStep::Repos && self.repo_filter_active => {
                self.repo_filter.handle_key(key);
                self.clamp_active_cursor();
                self.errors.clear();
            }
            KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Char('a')
            | KeyCode::Char('e')
            | KeyCode::Char('u')
            | KeyCode::Char('k')
            | KeyCode::Char('w')
                if step == CreateStep::Repos && self.repo_filter_active =>
            {
                self.repo_filter.handle_key(key);
                self.clamp_active_cursor();
                self.errors.clear();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_active_cursor(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_active_cursor(-1);
            }
            KeyCode::Char('n') if ctrl => {
                self.move_active_cursor(1);
            }
            KeyCode::Char('p') if ctrl => {
                self.move_active_cursor(-1);
            }
            KeyCode::Right if step == CreateStep::AfterCreate => {
                self.move_after_create_cursor(1);
            }
            KeyCode::Left if step == CreateStep::AfterCreate => {
                self.move_after_create_cursor(-1);
            }
            _ => {}
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        match CreateWizardLayout::for_width(area.width) {
            CreateWizardLayout::TooNarrow => self.render_too_narrow(frame, area),
            CreateWizardLayout::TwoColumn => self.render_two_column(frame, area),
            CreateWizardLayout::SingleColumn => self.render_single_column(frame, area),
        }
    }

    fn should_quit(&self) -> bool {
        self.outcome.borrow().is_some()
    }
}

pub fn run_create_wizard(
    draft: CreateDraft,
    global: GlobalConfig,
    existing_workspaces: Vec<String>,
) -> anyhow::Result<CreateWizardOutput> {
    let outcome = Rc::new(RefCell::new(None));
    let app =
        CreateWizardApp::with_outcome_handle(draft, global, existing_workspaces, outcome.clone());
    run_app(app)?;
    let outcome = outcome.borrow().clone();
    match outcome {
        Some(CreateWizardOutcome::Submit(output)) => Ok(output),
        Some(CreateWizardOutcome::Cancelled) | None => Err(CancelledByUser.into()),
    }
}
