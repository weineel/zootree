use anyhow::Result;
use dialoguer::{Confirm, Input, MultiSelect, Select};

pub fn input_required(prompt: &str) -> Result<String> {
    let value: String = Input::new().with_prompt(prompt).interact_text()?;
    Ok(value)
}

pub fn input_optional(prompt: &str) -> Result<Option<String>> {
    let value: String = Input::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text()?;
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

pub fn select_one(prompt: &str, items: &[String]) -> Result<usize> {
    let selection = Select::new().with_prompt(prompt).items(items).interact()?;
    Ok(selection)
}

pub fn select_multi(prompt: &str, items: &[String]) -> Result<Vec<usize>> {
    let selections = MultiSelect::new()
        .with_prompt(prompt)
        .items(items)
        .interact()?;
    Ok(selections)
}

pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let result = Confirm::new()
        .with_prompt(prompt)
        .default(default)
        .interact()?;
    Ok(result)
}
