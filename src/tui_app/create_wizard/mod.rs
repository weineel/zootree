use std::cell::RefCell;
use std::rc::Rc;

use crate::cli::create_flow::{CreateDraft, CreateWizardOutput};
use crate::config::global::GlobalConfig;
use crate::tui_app::{run_app, CancelledByUser};

mod navigation;
mod render;
mod repo_page;
mod state;
mod text_field;

pub use repo_page::{repo_list_label, review_repo_label};
pub use state::{CreateStep, CreateWizardApp, CreateWizardPage};

use text_field::{WizardTextField, WizardTextKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateWizardOutcome {
    Submit(Box<CreateWizardOutput>),
    Cancelled,
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
        Some(CreateWizardOutcome::Submit(output)) => Ok(*output),
        Some(CreateWizardOutcome::Cancelled) | None => Err(CancelledByUser.into()),
    }
}
