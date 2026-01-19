use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "cxn",
    about = "A CLI tool for quick ping and DNS connectivity checks",
    version = env!("GIT_DESCRIBE"),
    after_help = "Logs are written to: ~/.local/share/cxn/logs/cxn.log"
)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, global = true, help = "Path to config file")]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long, global = true, help = "Enable verbose output")]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Ping a host to check reachability
    Ping {
        /// Host to ping (IP address or hostname)
        #[arg(required = true)]
        host: String,

        /// Number of ping attempts
        #[arg(short = 'n', long, default_value = "4")]
        count: u32,

        /// Timeout in milliseconds
        #[arg(short, long, default_value = "1000")]
        timeout: u64,
    },

    /// Resolve DNS for a hostname
    Dns {
        /// Hostname to resolve
        #[arg(required = true)]
        hostname: String,

        /// Include IPv6 addresses
        #[arg(short = '6', long)]
        ipv6: bool,
    },

    /// Check connectivity for all configured hosts (default)
    Check {
        /// Run checks sequentially instead of in parallel
        #[arg(short, long)]
        sequential: bool,
    },
}
