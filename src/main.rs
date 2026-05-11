use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::CompleteEnv;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, Layer};
use zootree::cli::{Cli, Commands};

fn init_tracing(
    verbose: bool,
    quiet: bool,
) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot find config directory"))?
        .join("zootree/logs");
    std::fs::create_dir_all(&config_dir)?;

    let file_appender = rolling::daily(&config_dir, "zootree.log");
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

    let _guard = match init_tracing(cli.verbose, cli.quiet) {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("Error: failed to initialize tracing: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = run(cli.command) {
        tracing::error!("{:#}", e);
        std::process::exit(1);
    }
}

fn run(command: Commands) -> Result<()> {
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
            let config_dir = dirs::config_dir()
                .ok_or_else(|| anyhow::anyhow!("cannot find config directory"))?
                .join("zootree/logs/zootree.log");
            if config_dir.exists() {
                let status = std::process::Command::new("tail")
                    .args(["-f", "-n", "100"])
                    .arg(&config_dir)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("tail exited with error");
                }
            } else {
                println!("no log file found at {}", config_dir.display());
            }
        }
        Commands::Completions(args) => {
            zootree::cli::completions::handle_completions(&args)?;
        }
    }

    Ok(())
}
