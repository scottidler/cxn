use clap::Parser;
use colored::*;
use comfy_table::{presets::NOTHING, Cell, CellAlignment, Color, Table};
use eyre::{Context, Result};
use log::info;
use std::fs;
use std::io::{self, Write};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::signal;

mod check;
mod cli;
mod config;
mod dns;
mod ping;

use cli::{Cli, Commands};
use config::Config;

/// Resolve watch interval with precedence: CLI > env > config > default
/// Returns None if watch mode not enabled, Some(interval) otherwise
fn resolve_watch_interval(cli_value: Option<u64>, config: &Config) -> Option<u64> {
    match cli_value {
        None => None, // --watch not specified
        Some(0) => {
            // --watch with no value, use config/env
            let env_val = std::env::var("CXN_WATCH_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok());
            Some(env_val.unwrap_or(config.interval))
        }
        Some(n) => Some(n), // --watch N, use explicit value
    }
}

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

/// Handle the `cxn ping` subcommand
async fn cmd_ping(host: &str, count: u32, timeout_ms: u64) -> Result<()> {
    // Parse or resolve the host to an IP address
    let address: IpAddr = if let Ok(ip) = host.parse() {
        ip
    } else {
        // Need to resolve hostname first
        let resolver = dns::create_resolver();
        let result = dns::resolve_dns(&resolver, host, host, false).await;
        if !result.success {
            eprintln!(
                "{}: {} - {}",
                "Error".red(),
                host,
                result.error.unwrap_or_else(|| "DNS resolution failed".to_string())
            );
            std::process::exit(1);
        }
        result
            .addresses
            .into_iter()
            .next()
            .ok_or_else(|| eyre::eyre!("No IP addresses found for {}", host))?
    };

    let client = ping::create_client()?;
    let timeout = Duration::from_millis(timeout_ms);
    let result = ping::ping_host_detailed(&client, address, timeout, count).await;
    println!("{}", result.format());

    if result.packets_received == 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Handle the `cxn dns` subcommand
async fn cmd_dns(hostname: &str, include_ipv6: bool) -> Result<()> {
    let resolver = dns::create_resolver();
    let result = dns::resolve_dns_detailed(&resolver, hostname, include_ipv6).await;
    println!("{}", result.format());

    if result.error.is_some() {
        std::process::exit(1);
    }

    Ok(())
}

/// Handle the `cxn check` subcommand (default) - verbose output
/// Returns true if all checks passed, false otherwise
async fn cmd_check(config: &Config, sequential: bool) -> Result<bool> {
    let hosts = config.hosts();
    if hosts.is_empty() {
        println!("{}", "No hosts configured".yellow());
        println!("Add hosts to ~/.config/cxn/cxn.yml or ./cxn.yml to get started.");
        return Ok(true);
    }

    let start_time = Instant::now();
    println!("Checking {} hosts...\n", hosts.len());

    // Create shared clients
    let ping_client = Arc::new(ping::create_client()?);
    let dns_resolver = Arc::new(dns::create_resolver());

    // Run checks (parallel by default)
    let parallel = !sequential;
    let results = check::run_all_checks(config, ping_client, dns_resolver, parallel).await;

    // Display results
    let mut success_count = 0;
    for result in &results {
        println!("{} ({})", result.name.cyan(), result.address);

        if let Some(ref dns_result) = result.dns {
            println!("{}", dns_result.format());
        }

        if let Some(ref ping_result) = result.ping {
            println!("{}", ping_result.format());
        }

        if result.is_success() {
            success_count += 1;
        }

        println!();
    }

    // Summary
    let elapsed = start_time.elapsed();
    let hosts_checked = hosts.iter().filter(|h| h.has_checks()).count();
    if success_count == hosts_checked {
        println!(
            "Summary: {}/{} hosts {} in {:.1}s",
            success_count,
            hosts_checked,
            "OK".green(),
            elapsed.as_secs_f64()
        );
        Ok(true)
    } else {
        let failed = hosts_checked - success_count;
        println!(
            "Summary: {}/{} hosts OK, {} {} in {:.1}s",
            success_count,
            hosts_checked,
            failed,
            "failed".red(),
            elapsed.as_secs_f64()
        );
        Ok(false)
    }
}

/// Handle check in compact table format for watch mode
async fn cmd_check_compact(config: &Config, sequential: bool) -> Result<bool> {
    let hosts = config.hosts();
    if hosts.is_empty() {
        println!("{}", "No hosts configured".yellow());
        return Ok(true);
    }

    // Create shared clients
    let ping_client = Arc::new(ping::create_client()?);
    let dns_resolver = Arc::new(dns::create_resolver());

    // Run checks
    let parallel = !sequential;
    let results = check::run_all_checks(config, ping_client, dns_resolver, parallel).await;

    // Build table
    let mut table = Table::new();
    table.load_preset(NOTHING);

    // Header
    table.set_header(vec![
        Cell::new("NAME").fg(Color::DarkGrey),
        Cell::new("PING").fg(Color::DarkGrey).set_alignment(CellAlignment::Right),
        Cell::new("DNS").fg(Color::DarkGrey),
    ]);

    // Results
    let mut success_count = 0;
    for result in &results {
        let (ping_text, ping_color) = match &result.ping {
            Some(p) if p.success && p.rtt.is_some() => {
                (format!("{:.1}ms", p.rtt.unwrap().as_secs_f64() * 1000.0), Color::Green)
            }
            Some(p) if p.success => ("ok".to_string(), Color::Green),
            Some(_) => ("fail".to_string(), Color::Red),
            None => ("-".to_string(), Color::DarkGrey),
        };

        let (dns_text, dns_color) = match &result.dns {
            Some(d) if d.success => {
                let addr = d.addresses.first().map(|a| a.to_string()).unwrap_or_default();
                (addr, Color::Green)
            }
            Some(_) => ("fail".to_string(), Color::Red),
            None => ("-".to_string(), Color::DarkGrey),
        };

        let name_color = if result.is_success() { Color::Reset } else { Color::Red };

        table.add_row(vec![
            Cell::new(&result.name).fg(name_color),
            Cell::new(ping_text).fg(ping_color).set_alignment(CellAlignment::Right),
            Cell::new(dns_text).fg(dns_color),
        ]);

        if result.is_success() {
            success_count += 1;
        }
    }

    println!("{table}");
    println!();
    io::stdout().flush().ok();

    let hosts_checked = hosts.iter().filter(|h| h.has_checks()).count();
    Ok(success_count == hosts_checked)
}

/// Run check command with optional watch mode
async fn run_check_with_watch(config: &Config, sequential: bool, watch: Option<u64>) -> Result<()> {
    let interval = resolve_watch_interval(watch, config);

    match interval {
        None => {
            // Single run mode
            let success = cmd_check(config, sequential).await?;
            if !success {
                std::process::exit(1);
            }
        }
        Some(seconds) => {
            // Watch mode - true fixed interval from cycle start
            let interval_duration = Duration::from_secs(seconds);

            loop {
                let cycle_start = Instant::now();

                // Clear screen and move cursor to top-left
                print!("\x1B[2J\x1B[1;1H");
                io::stdout().flush().ok();

                let now = chrono::Local::now();
                println!(
                    "{} [{}] (every {}s)\n",
                    "cxn".cyan().bold(),
                    now.format("%H:%M:%S"),
                    seconds
                );

                // Run the compact check
                let _ = cmd_check_compact(config, sequential).await?;

                // Calculate remaining time in interval
                let elapsed = cycle_start.elapsed();
                let remaining = interval_duration.saturating_sub(elapsed);

                // Wait for remaining interval or Ctrl+C
                tokio::select! {
                    _ = tokio::time::sleep(remaining) => {
                        // Continue to next iteration
                    }
                    _ = signal::ctrl_c() => {
                        println!("\n\n{}", "Watch mode stopped.".yellow());
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging first
    setup_logging().context("Failed to setup logging")?;

    // Parse CLI arguments
    let cli = Cli::parse();

    info!("Starting with config from: {:?}", cli.config);

    // Dispatch to the appropriate command
    match cli.command {
        Some(Commands::Ping { host, count, timeout }) => {
            cmd_ping(&host, count, timeout).await?;
        }
        Some(Commands::Dns { hostname, ipv6 }) => {
            cmd_dns(&hostname, ipv6).await?;
        }
        Some(Commands::Check { sequential, watch }) => {
            // Load configuration for check command
            let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;
            run_check_with_watch(&config, sequential, watch).await?;
        }
        None => {
            // Default: run check command with parallel execution (no watch)
            let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;
            run_check_with_watch(&config, false, None).await?;
        }
    }

    Ok(())
}
