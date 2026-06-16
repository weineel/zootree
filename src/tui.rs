//! Public prompt API. Implementations live in `crate::tui_app::prompt` and
//! are run via `crate::tui_app::run_inline`.

use anyhow::Result;

use crate::tui_app::prompt::{
    ConfirmPromptState, MultiSelectPromptState, SelectPromptState, TextPromptState,
};
use crate::tui_app::{run_inline, CancelledByUser, PromptOutcome};

pub fn input_required(prompt: &str) -> Result<String> {
    let state = TextPromptState::new(prompt).required();
    match run_inline(state)? {
        PromptOutcome::Submitted(s) => Ok(s),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("required text prompt cannot be skipped"),
    }
}

pub fn input_optional(prompt: &str) -> Result<Option<String>> {
    let state = TextPromptState::new(prompt).optional();
    match run_inline(state)? {
        PromptOutcome::Submitted(s) if s.is_empty() => Ok(None),
        PromptOutcome::Submitted(s) => Ok(Some(s)),
        PromptOutcome::Skipped => Ok(None),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
    }
}

pub fn select_one(prompt: &str, items: &[String]) -> Result<usize> {
    let state = SelectPromptState::new(prompt, items.to_vec());
    match run_inline(state)? {
        PromptOutcome::Submitted(idx) => Ok(idx),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("select_one is required"),
    }
}

pub fn select_multi(prompt: &str, items: &[String]) -> Result<Vec<usize>> {
    let state = MultiSelectPromptState::new(prompt, items.to_vec());
    match run_inline(state)? {
        PromptOutcome::Submitted(v) => Ok(v),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("select_multi is required"),
    }
}

pub fn select_multi_with_defaults(
    prompt: &str,
    items: &[String],
    default_indices: &[usize],
) -> Result<Vec<usize>> {
    let state = MultiSelectPromptState::with_defaults(prompt, items.to_vec(), default_indices);
    match run_inline(state)? {
        PromptOutcome::Submitted(v) => Ok(v),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("select_multi is required"),
    }
}

pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let state = ConfirmPromptState::new(prompt, default);
    match run_inline(state)? {
        PromptOutcome::Submitted(v) => Ok(v),
        PromptOutcome::Aborted | PromptOutcome::Interrupted => Err(CancelledByUser.into()),
        PromptOutcome::Skipped => unreachable!("confirm is required"),
    }
}
