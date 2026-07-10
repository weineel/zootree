use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::cli::create_flow::{AfterCreateMode, CreateDraftError, CreateWizardOutput};
use crate::tui_app::{App, Event};

use super::{CreateStep, CreateWizardApp, CreateWizardOutcome, CreateWizardPage, WizardTextField};

impl CreateWizardApp {
    fn error_belongs_to_step(step: CreateStep, err: &CreateDraftError) -> bool {
        match step {
            CreateStep::Info => matches!(
                err,
                CreateDraftError::TitleRequired
                    | CreateDraftError::TitleSingleLineRequired
                    | CreateDraftError::WorkspaceNameRequired
                    | CreateDraftError::WorkspaceNameInvalid(_)
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

    pub(super) fn refresh_current_step_errors(&mut self) {
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
            *self.outcome.borrow_mut() =
                Some(CreateWizardOutcome::Submit(Box::new(CreateWizardOutput {
                    draft: self.draft.clone(),
                })));
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

    fn handle_text_key(&mut self, key: KeyEvent) -> bool {
        if let Some(field) = &mut self.text_field {
            field.handle_key(key);
            self.errors.clear();
            return true;
        }
        false
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
        let alt = key.modifiers.contains(KeyModifiers::ALT);
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
            KeyCode::Down | KeyCode::Char('j') if step == CreateStep::Review && alt => {
                self.scroll_draft_down(1);
            }
            KeyCode::Up | KeyCode::Char('k') if step == CreateStep::Review && alt => {
                self.scroll_draft_up(1);
            }
            KeyCode::PageDown if step == CreateStep::Review && alt => {
                self.scroll_draft_down(self.draft_page_scroll_amount());
            }
            KeyCode::PageUp if step == CreateStep::Review && alt => {
                self.scroll_draft_up(self.draft_page_scroll_amount());
            }
            KeyCode::Home if step == CreateStep::Review && alt => {
                self.draft_scroll = 0;
            }
            KeyCode::End if step == CreateStep::Review && alt => {
                self.draft_scroll = usize::MAX;
            }
            KeyCode::Down | KeyCode::Char('j') if step == CreateStep::Review => {
                self.scroll_review_down(1);
            }
            KeyCode::Up | KeyCode::Char('k') if step == CreateStep::Review => {
                self.scroll_review_up(1);
            }
            KeyCode::PageDown if step == CreateStep::Review => {
                self.scroll_review_down(self.review_page_scroll_amount());
            }
            KeyCode::PageUp if step == CreateStep::Review => {
                self.scroll_review_up(self.review_page_scroll_amount());
            }
            KeyCode::Home if step == CreateStep::Review => {
                self.review_scroll = 0;
            }
            KeyCode::End if step == CreateStep::Review => {
                self.review_scroll = usize::MAX;
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
        self.render_frame(frame);
    }

    fn should_quit(&self) -> bool {
        self.outcome.borrow().is_some()
    }
}
