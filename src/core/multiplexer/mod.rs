pub mod cmux;
pub mod zellij;

use crate::config::global::MultiplexerKind;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplexerLaunch {
    pub workspace_name: String,
    pub display_name: String,
    pub description: String,
    pub workspace_dir: PathBuf,
    pub layout_name: String,
    pub rendered_layout: String,
    pub layout_file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplexerIdentity {
    pub workspace_name: String,
    pub display_name: String,
    pub cmux_workspace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchOutcome {
    Launched,
    Attached,
    AlreadyRunning,
    BackgroundCreated,
}

pub trait TerminalMultiplexer {
    fn kind(&self) -> MultiplexerKind;
    fn launch(&self, launch: &MultiplexerLaunch) -> Result<LaunchOutcome>;
    fn open(
        &self,
        launch: &MultiplexerLaunch,
        identity: &MultiplexerIdentity,
    ) -> Result<LaunchOutcome>;
    fn close(&self, identity: &MultiplexerIdentity) -> Result<()>;
}
