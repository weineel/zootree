use crate::config::ConfigManager;
use crate::daemon::{self, client, DaemonOptions, DaemonState};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::time::{Duration, Instant};

#[derive(Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub command: DaemonCommands,
}

#[derive(Subcommand)]
pub enum DaemonCommands {
    #[command(about = "Start the daemon in the background")]
    Start,
    #[command(about = "Stop the running daemon")]
    Stop,
    #[command(about = "Show whether the daemon is running")]
    Status,
    /// Foreground entry point used by `daemon start`.
    #[command(hide = true)]
    Run,
}

pub fn handle_daemon_command(command: &DaemonCommands, config_mgr: &ConfigManager) -> Result<()> {
    let options = DaemonOptions::new(config_mgr.base_dir.clone());
    match command {
        DaemonCommands::Start => handle_start(&options),
        DaemonCommands::Stop => handle_stop(&options),
        DaemonCommands::Status => handle_status(&options),
        DaemonCommands::Run => {
            let handle = daemon::start(options)?;
            handle.join()
        }
    }
}

fn handle_status(options: &DaemonOptions) -> Result<()> {
    let paths = options.paths()?;
    match daemon::inspect(&paths)? {
        DaemonState::Running(discovery) => {
            let response = client::request(
                &discovery.endpoint,
                "GET",
                "/status",
                Some(&discovery.token),
                Duration::from_secs(5),
            )?;
            let status = response.json()?;
            println!(
                "daemon running: pid {}, version {}, protocol {}, started {}",
                discovery.pid,
                status["version"].as_str().unwrap_or("?"),
                status["protocol_version"],
                discovery.started_at
            );
            println!("endpoint: {}", discovery.endpoint.display());
        }
        DaemonState::NotRunning => println!("daemon not running"),
        DaemonState::Unknown(reason) => {
            println!("daemon state unknown ({reason}); it may be running")
        }
    }
    Ok(())
}

fn handle_start(options: &DaemonOptions) -> Result<()> {
    let paths = options.paths()?;
    match daemon::inspect(&paths)? {
        DaemonState::Running(d) => {
            anyhow::bail!("daemon is already running (pid {})", d.pid)
        }
        DaemonState::Unknown(reason) => anyhow::bail!(
            "cannot tell whether a daemon is already running ({reason}); not starting another"
        ),
        DaemonState::NotRunning => {}
    }

    paths.ensure_dir()?;
    let log_path = paths.dir.join(format!("{}.log", options.endpoint_name));
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    // A detached long-lived process, not a captured command, so this is the one
    // place that does not go through CommandRunner.
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .args(["daemon", "run"])
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .context("failed to spawn the daemon process")?;

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(exit) = child.try_wait()? {
            anyhow::bail!(
                "daemon exited during startup ({exit}); see {}",
                log_path.display()
            );
        }
        if let DaemonState::Running(d) = daemon::inspect(&paths)? {
            if d.pid == child.id() {
                println!("daemon started (pid {})", d.pid);
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    anyhow::bail!(
        "daemon did not become ready within 10s; see {}",
        log_path.display()
    )
}

fn handle_stop(options: &DaemonOptions) -> Result<()> {
    let paths = options.paths()?;
    match daemon::inspect(&paths)? {
        DaemonState::NotRunning => {
            println!("daemon not running");
            Ok(())
        }
        DaemonState::Unknown(reason) => {
            anyhow::bail!("cannot tell whether the daemon is running ({reason}); not stopping")
        }
        DaemonState::Running(d) => {
            client::request(
                &d.endpoint,
                "POST",
                "/shutdown",
                Some(&d.token),
                Duration::from_secs(5),
            )?;
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if matches!(daemon::inspect(&paths)?, DaemonState::NotRunning) {
                    println!("daemon stopped");
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            anyhow::bail!("daemon (pid {}) did not stop within 10s", d.pid)
        }
    }
}
