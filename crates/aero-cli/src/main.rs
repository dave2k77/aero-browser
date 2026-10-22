mod doctor;

use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "aero", version, about = "AeroBrowser local task browser")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect local prerequisites without launching a browsing session.
    Doctor {
        /// Emit a bounded, versioned JSON readiness report.
        #[arg(long)]
        json: bool,
        /// Explicit trusted browser executable; overrides AERO_CHROMIUM and PATH.
        #[arg(long)]
        chromium: Option<OsString>,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Commands::Doctor { json, chromium } => {
            let report = doctor::inspect(chromium.map(PathBuf::from));
            if json {
                match serde_json::to_string_pretty(&report) {
                    Ok(value) => println!("{value}"),
                    Err(_) => {
                        eprintln!("Cannot encode readiness report.");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                println!("Browser: {}", report.browser.description());
                println!("Bubblewrap: {}", report.bubblewrap.description());
                println!("Browser pin: {}", report.browser_lock.status);
                println!(
                    "Sandbox: {} — {}",
                    report.sandbox.status, report.sandbox.detail
                );
                println!("Runtime ready: {}", report.runtime_ready);
            }
            if report.runtime_ready {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
    }
}
