use std::cell::RefCell;
use std::rc::Rc;

use crate::cli::create_flow::{AfterCreateMode, CreateDraft, CreateDraftError, RepoDraftEntry};
use crate::config::global::GlobalConfig;

use super::{CreateWizardOutcome, WizardTextField, WizardTextKind};

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

pub struct CreateWizardApp {
    pub(super) draft: CreateDraft,
    pub(super) global: GlobalConfig,
    pub(super) existing_workspaces: Vec<String>,
    pub(super) pages: Vec<CreateWizardPage>,
    pub(super) page_index: usize,
    pub(super) repo_cursor: usize,
    pub(super) repo_filter: WizardTextField,
    pub(super) repo_filter_active: bool,
    pub(super) after_create_cursor: usize,
    pub(super) run_agent_value: Option<String>,
    pub(super) text_field: Option<WizardTextField>,
    pub(super) errors: Vec<CreateDraftError>,
    pub(super) review_scroll: usize,
    pub(super) draft_scroll: usize,
    pub(super) last_review_height: u16,
    pub(super) last_draft_height: u16,
    pub(super) outcome: Rc<RefCell<Option<CreateWizardOutcome>>>,
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
            review_scroll: 0,
            draft_scroll: 0,
            last_review_height: 1,
            last_draft_height: 1,
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

    pub(super) fn build_pages(draft: &CreateDraft) -> Vec<CreateWizardPage> {
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

    pub(super) fn page_text_kind(page: &CreateWizardPage) -> Option<WizardTextKind> {
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

    pub(super) fn step_for_page(page: &CreateWizardPage) -> CreateStep {
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

    pub(super) fn first_page_index_for_step(&self, step: CreateStep) -> usize {
        self.pages
            .iter()
            .position(|page| Self::step_for_page(page) == step)
            .unwrap_or(0)
    }

    pub(super) fn enter_page(&mut self, page_index: usize) {
        self.page_index = page_index.min(self.pages.len().saturating_sub(1));
        self.errors.clear();
        self.sync_current_page_state(true);
    }

    pub(super) fn sync_current_page_state(&mut self, _reset_editing: bool) {
        self.text_field = self
            .current_page_text()
            .map(|(text, kind)| WizardTextField::new(text, kind));
    }

    pub(super) fn current_page_text(&self) -> Option<(String, WizardTextKind)> {
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

    pub(super) fn clamp_page_index(&mut self) {
        if self.pages.is_empty() {
            self.page_index = 0;
        } else {
            self.page_index = self.page_index.min(self.pages.len() - 1);
        }
    }

    pub(super) fn refresh_pages(&mut self) {
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

    fn after_create_cursor_for_mode(mode: &AfterCreateMode) -> usize {
        match mode {
            AfterCreateMode::CreateOnly => 0,
            AfterCreateMode::Start => 1,
            AfterCreateMode::StartAndRunAgent { .. } => 2,
        }
    }

    pub(super) fn repo_mut(&mut self, name: &str) -> Option<&mut RepoDraftEntry> {
        self.draft.repos.iter_mut().find(|repo| repo.name == name)
    }
}
