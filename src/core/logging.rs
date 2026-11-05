use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing_appender::rolling::{self, RollingFileAppender};

use crate::config::{global::GlobalConfig, ConfigManager};

pub const LOG_FILE_NAME: &str = "zootree.log";

pub fn default_log_dir(config_mgr: &ConfigManager) -> PathBuf {
    config_mgr.base_dir.join("logs")
}

pub fn resolve_log_dir(config_mgr: &ConfigManager, global: &GlobalConfig) -> PathBuf {
    global
        .log
        .dir
        .as_deref()
        .map(expand_log_dir)
        .unwrap_or_else(|| default_log_dir(config_mgr))
}

pub fn resolve_log_file_path(config_mgr: &ConfigManager, global: &GlobalConfig) -> PathBuf {
    resolve_log_dir(config_mgr, global).join(LOG_FILE_NAME)
}

fn expand_log_dir(dir: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(dir).into_owned())
}

const SYMLINK_ATTEMPTS: u32 = 5;

/// Build the daily rolling log appender.
///
/// The "latest" symlink is replaced by remove-then-create, so several zootree
/// processes starting at once (e.g. a GUI polling `list --json`) can collide
/// with "File exists". Logging must never stop a command from running, so
/// retry briefly and then fall back to an appender without the symlink.
pub fn build_file_appender(
    log_dir: &Path,
    max_files: Option<u32>,
) -> anyhow::Result<RollingFileAppender> {
    let build = |with_symlink: bool| {
        let mut builder = rolling::RollingFileAppender::builder()
            .rotation(rolling::Rotation::DAILY)
            .filename_prefix(LOG_FILE_NAME);
        if with_symlink {
            builder = builder.latest_symlink(LOG_FILE_NAME);
        }
        if let Some(max_files) = max_files {
            builder = builder.max_log_files(max_files as usize);
        }
        builder.build(log_dir)
    };

    for attempt in 0..SYMLINK_ATTEMPTS {
        if let Ok(appender) = build(true) {
            return Ok(appender);
        }
        std::thread::sleep(Duration::from_millis(10 * u64::from(attempt + 1)));
    }
    Ok(build(false)?)
}
