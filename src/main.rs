mod build;
mod project;
mod release;
mod render;

use clap::{CommandFactory, Parser, Subcommand};
use std::process::ExitCode;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Parser)]
#[command(name = "shipwright", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Prepare and publish packages and optional host-platform binary downloads
    Release {
        /// Release only this configured package (python, typescript, go, or rust)
        package: Option<String>,
        /// Prepare and preview without changing registries, remote tags, or GitHub
        #[arg(long)]
        dry_run: bool,
        /// Reuse build outputs; still pack npm and Cargo source packages
        #[arg(long)]
        skip_build: bool,
    },
    /// Build the source and targets concurrently using their existing tooling
    Build {
        /// Build only this package (python, typescript, go, or rust)
        package: Option<String>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<u8, Box<dyn std::error::Error>> {
    let Some(command) = cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(0);
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancelled);
    ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
    let package = match command {
        Commands::Release {
            package,
            dry_run,
            skip_build,
        } => {
            let result = release::run(
                &std::env::current_dir()?,
                package.as_deref(),
                dry_run,
                skip_build,
                &cancelled,
            );
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(130);
            }
            return result.map_err(Into::into);
        }
        Commands::Build { package } => package,
    };
    let packages = project::discover(&std::env::current_dir()?, package.as_deref())?;
    // TODO: Provide cleanup for retained build logs so repeated runs do not accumulate forever.
    let logs = tempfile::Builder::new()
        .prefix("shipwright-build-")
        .tempdir()?
        .keep();
    let display = render::Display::new(&packages);
    let results = build::run(&packages, &logs, &cancelled, |index, result| {
        display.complete(index, result);
    });
    display.finish(&results, &logs);
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        Ok(130)
    } else if results.iter().any(|result| !result.succeeded()) {
        Ok(1)
    } else {
        Ok(0)
    }
}
