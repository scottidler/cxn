use clap::Parser;
use colored::*;
use eyre::{Context, Result};
use log::info;
use std::fs;
use std::path::PathBuf;

mod cli;
mod config;
mod dns;
mod ping;

use cli::Cli;
use config::Config;

fn setup_logging() -> Result<()> {
    // Create log directory
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cxn")
        .join("logs");

    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    let log_file = log_dir.join("cxn.log");

    // Setup env_logger with file output
    let target = Box::new(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .context("Failed to open log file")?,
    );

    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(target))
        .init();

    info!("Logging initialized, writing to: {}", log_file.display());
    Ok(())
}

async fn run_application(_cli: &Cli, config: &Config) -> Result<()> {
    info!("Starting application");

    // Load and display configuration
    println!("{}", "Configuration loaded successfully".green());

    if config.hosts.is_empty() {
        println!("{}", "No hosts configured".yellow());
        println!("Add hosts to ~/.config/cxn/cxn.yml or ./cxn.yml to get started.");
        return Ok(());
    }

    println!(
        "Found {} host(s) configured with timeout {}ms",
        config.hosts.len(),
        config.timeout_ms
    );

    for host in &config.hosts {
        let checks: Vec<&str> = [
            if host.ping { Some("ping") } else { None },
            if host.dns { Some("dns") } else { None },
        ]
        .into_iter()
        .flatten()
        .collect();

        println!("  {} ({}) - {}", host.name.cyan(), host.address, checks.join(", "));
    }

    info!("Application executed successfully");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging first
    setup_logging().context("Failed to setup logging")?;

    // Parse CLI arguments
    let cli = Cli::parse();

    // Load configuration
    let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;

    info!("Starting with config from: {:?}", cli.config);

    // Run the main application logic
    run_application(&cli, &config).await.context("Application failed")?;

    Ok(())
}
