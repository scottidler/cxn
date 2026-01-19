use clap::Parser;
use colored::*;
use eyre::{Context, Result};
use log::info;
use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

mod cli;
mod config;
mod dns;
mod ping;

use cli::{Cli, Commands};
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

/// Handle the `cxn check` subcommand (default)
async fn cmd_check(config: &Config, _sequential: bool) -> Result<()> {
    if config.hosts.is_empty() {
        println!("{}", "No hosts configured".yellow());
        println!("Add hosts to ~/.config/cxn/cxn.yml or ./cxn.yml to get started.");
        return Ok(());
    }

    println!("Checking {} hosts...\n", config.hosts.len());

    // For now, run checks sequentially (parallel will be added in Phase 5)
    let timeout = Duration::from_millis(config.timeout_ms);
    let ping_client = ping::create_client()?;
    let dns_resolver = dns::create_resolver();

    let mut success_count = 0;

    for host in &config.hosts {
        println!("{} ({})", host.name.cyan(), host.address);

        let mut host_success = true;

        // DNS check (only if enabled and address is a hostname)
        if host.should_resolve_dns() {
            let dns_result = dns::resolve_dns(&dns_resolver, &host.name, &host.address, true).await;
            println!("{}", dns_result.format());
            if !dns_result.success {
                host_success = false;
            }
        }

        // Ping check
        if host.ping {
            // Parse or resolve the address
            let ip_address: Option<IpAddr> = if let Ok(ip) = host.address.parse() {
                Some(ip)
            } else {
                // Need to resolve first
                let dns_result = dns::resolve_dns(&dns_resolver, &host.name, &host.address, false).await;
                dns_result.addresses.into_iter().next()
            };

            if let Some(addr) = ip_address {
                let ping_result = ping::ping_host(&ping_client, &host.name, addr, timeout, 1).await;
                println!("{}", ping_result.format());
                if !ping_result.success {
                    host_success = false;
                }
            } else {
                println!("  {} ping: could not resolve hostname", "✗".red());
                host_success = false;
            }
        }

        if host_success {
            success_count += 1;
        }

        println!();
    }

    // Summary
    let hosts_checked = config.hosts.iter().filter(|h| h.has_checks()).count();
    if success_count == hosts_checked {
        println!("Summary: {}/{} hosts {}", success_count, hosts_checked, "OK".green());
    } else {
        let failed = hosts_checked - success_count;
        println!(
            "Summary: {}/{} hosts OK, {} {}",
            success_count,
            hosts_checked,
            failed,
            "failed".red()
        );
        std::process::exit(1);
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
        Some(Commands::Check { sequential }) => {
            // Load configuration for check command
            let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;
            cmd_check(&config, sequential).await?;
        }
        None => {
            // Default: run check command with parallel execution
            let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;
            cmd_check(&config, false).await?;
        }
    }

    Ok(())
}
