use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::CompleteEnv;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, Layer};
use zootree::cli::{Cli, Commands};
use zootree::config::{global::GlobalConfig, ConfigManager};
use zootree::core::logging::{resolve_log_dir, resolve_log_file_path, LOG_FILE_NAME};

fn init_tracing(
    verbose: bool,
    quiet: bool,
    config_mgr: &ConfigManager,
    global: &GlobalConfig,
) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = resolve_log_dir(config_mgr, global);
    std::fs::create_dir_all(&log_dir)?;

    let mut file_appender = rolling::RollingFileAppender::builder()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix(LOG_FILE_NAME)
        .latest_symlink(LOG_FILE_NAME);
    if let Some(max_files) = global.log.max_files {
        file_appender = file_appender.max_log_files(max_files as usize);
    }
    let file_appender = file_appender.build(&log_dir)?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let terminal_level = if quiet {
        "error"
    } else if verbose {
        "debug"
    } else {
        "info"
    };

    let terminal_layer = fmt::layer()
        .with_target(false)
        .with_level(true)
        .with_filter(tracing_subscriber::EnvFilter::new(terminal_level));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_filter(tracing_subscriber::EnvFilter::new("debug"));

    tracing_subscriber::registry()
        .with(terminal_layer)
        .with(file_layer)
        .init();

    Ok(guard)
}

fn main() {
    // Dynamic completion interceptor: if the COMPLETE env var is set, this
    // resolves the candidates and exits before any other side effects (no tracing,
    // no log files). Must run before Cli::parse().
    CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();

    let config_mgr = match ConfigManager::new() {
        Ok(config_mgr) => config_mgr,
        Err(e) => {
            eprintln!("Error: failed to initialize config: {}", e);
            std::process::exit(1);
        }
    };
    let global = match config_mgr.load_global_config() {
        Ok(global) => global,
        Err(e) => {
            eprintln!("Error: failed to load global config: {}", e);
            std::process::exit(1);
        }
    };

    let _guard = match init_tracing(cli.verbose, cli.quiet, &config_mgr, &global) {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("Error: failed to initialize tracing: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = run(cli.command, &config_mgr, &global) {
        if e.downcast_ref::<zootree::tui_app::CancelledByUser>()
            .is_some()
        {
            eprintln!("aborted");
            std::process::exit(1);
        }
        tracing::error!("{:#}", e);
        std::process::exit(1);
    }
}

fn run(command: Commands, config_mgr: &ConfigManager, global: &GlobalConfig) -> Result<()> {
    match command {
        Commands::Repo(args) => {
            zootree::cli::repo::handle_repo_command(&args.command)?;
        }
        Commands::Create(args) => {
            zootree::cli::workspace::handle_create(&args)?;
        }
        Commands::List(args) => {
            zootree::cli::workspace::handle_list(&args)?;
        }
        Commands::Start(args) => {
            zootree::cli::workspace::handle_start(&args)?;
        }
        Commands::Open(args) => {
            zootree::cli::workspace::handle_open(&args)?;
        }
        Commands::Info(args) => {
            zootree::cli::info::handle_info(&args)?;
        }
        Commands::Done(args) => {
            zootree::cli::workspace::handle_done(&args)?;
        }
        Commands::Cancel(args) => {
            zootree::cli::workspace::handle_cancel(&args)?;
        }
        Commands::Template(args) => {
            zootree::cli::template::handle_template_command(&args.command)?;
        }
        Commands::Prune(args) => {
            zootree::cli::prune::handle_prune(&args)?;
        }
        Commands::Logs => {
            let log_file = resolve_log_file_path(config_mgr, global);
            if log_file.exists() {
                let status = std::process::Command::new("tail")
                    .args(["-f", "-n", "100"])
                    .arg(&log_file)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("tail exited with error");
                }
            } else {
                println!("no log file found at {}", log_file.display());
            }
        }
        Commands::Completions(args) => {
            zootree::cli::completions::handle_completions(&args)?;
        }
    }

    Ok(())
}
