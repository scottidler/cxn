use colored::*;
use eyre::{Context, Result};
use rand::random;
use std::net::IpAddr;
use std::time::Duration;
use surge_ping::{Client, Config as PingConfig, PingIdentifier, PingSequence};

/// Result of a ping operation
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used in later phases
pub struct PingResult {
    /// Display name from config
    pub name: String,
    /// The IP address that was pinged
    pub address: IpAddr,
    /// Whether the ping was successful
    pub success: bool,
    /// Round-trip time if successful
    pub rtt: Option<Duration>,
    /// Error message if failed
    pub error: Option<String>,
}

#[allow(dead_code)] // Used in later phases
impl PingResult {
    /// Create a successful ping result
    pub fn success(name: String, address: IpAddr, rtt: Duration) -> Self {
        Self {
            name,
            address,
            success: true,
            rtt: Some(rtt),
            error: None,
        }
    }

    /// Create a failed ping result
    pub fn failure(name: String, address: IpAddr, error: String) -> Self {
        Self {
            name,
            address,
            success: false,
            rtt: None,
            error: Some(error),
        }
    }

    /// Format the result for display
    pub fn format(&self) -> String {
        if self.success {
            let rtt_str = self
                .rtt
                .map(|d| format!("{:.1}ms", d.as_secs_f64() * 1000.0))
                .unwrap_or_else(|| "?".to_string());
            format!("  {} ping: {}", "✓".green(), rtt_str)
        } else {
            let err_str = self.error.as_deref().unwrap_or("unknown error");
            format!("  {} ping: {}", "✗".red(), err_str)
        }
    }
}

/// Create a new ping client
#[allow(dead_code)] // Used in later phases
pub fn create_client() -> Result<Client> {
    Client::new(&PingConfig::default()).context("Failed to create ping client")
}

/// Ping a host and return the result
///
/// Sends ICMP echo requests to the specified address and measures RTT.
/// Returns the average RTT on success.
#[allow(dead_code)] // Used in later phases
pub async fn ping_host(client: &Client, name: &str, address: IpAddr, timeout: Duration, count: u32) -> PingResult {
    let mut rtts = Vec::with_capacity(count as usize);
    let mut last_error = None;

    // Generate a random identifier for this ping session
    let identifier = PingIdentifier(random());

    // Create pinger for this address
    let mut pinger = client.pinger(address, identifier).await;
    pinger.timeout(timeout);

    let payload = [0u8; 56]; // Standard ping payload size

    for seq in 0..count {
        match pinger.ping(PingSequence(seq as u16), &payload).await {
            Ok((_, rtt)) => {
                rtts.push(rtt);
            }
            Err(e) => {
                last_error = Some(format_ping_error(&e, timeout));
            }
        }
    }

    if rtts.is_empty() {
        // All pings failed
        PingResult::failure(
            name.to_string(),
            address,
            last_error.unwrap_or_else(|| "all pings failed".to_string()),
        )
    } else {
        // Calculate average RTT
        let avg_rtt = rtts.iter().sum::<Duration>() / rtts.len() as u32;
        PingResult::success(name.to_string(), address, avg_rtt)
    }
}

/// Format a ping error into a user-friendly message
#[allow(dead_code)] // Used in later phases
fn format_ping_error(error: &surge_ping::SurgeError, timeout: Duration) -> String {
    match error {
        surge_ping::SurgeError::Timeout { .. } => {
            format!("timeout after {}ms", timeout.as_millis())
        }
        surge_ping::SurgeError::IOError(io_err) => {
            if io_err.kind() == std::io::ErrorKind::PermissionDenied {
                "permission denied (need cap_net_raw)".to_string()
            } else if io_err.raw_os_error() == Some(101) {
                "network unreachable".to_string()
            } else if io_err.raw_os_error() == Some(113) {
                "no route to host".to_string()
            } else {
                format!("io error: {}", io_err)
            }
        }
        _ => format!("{}", error),
    }
}

/// Detailed ping output for the `cxn ping` subcommand
#[allow(dead_code)] // Used in later phases
pub struct DetailedPingResult {
    pub address: IpAddr,
    pub results: Vec<(u16, Result<Duration, String>)>,
    pub packets_sent: u32,
    pub packets_received: u32,
}

#[allow(dead_code)] // Used in later phases
impl DetailedPingResult {
    /// Format detailed output similar to traditional ping command
    pub fn format(&self) -> String {
        let mut output = vec![format!("PING {}", self.address)];

        for (seq, result) in &self.results {
            match result {
                Ok(rtt) => {
                    output.push(format!(
                        "  64 bytes: seq={} time={:.1}ms",
                        seq,
                        rtt.as_secs_f64() * 1000.0
                    ));
                }
                Err(e) => {
                    output.push(format!("  seq={}: {}", seq, e.red()));
                }
            }
        }

        output.push(String::new());
        output.push(format!("--- {} ping statistics ---", self.address));
        output.push(format!(
            "{} packets transmitted, {} received, {:.0}% packet loss",
            self.packets_sent,
            self.packets_received,
            if self.packets_sent > 0 {
                ((self.packets_sent - self.packets_received) as f64 / self.packets_sent as f64) * 100.0
            } else {
                0.0
            }
        ));

        if self.packets_received > 0 {
            let rtts: Vec<f64> = self
                .results
                .iter()
                .filter_map(|(_, r)| r.as_ref().ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .collect();

            if !rtts.is_empty() {
                let min = rtts.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = rtts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let avg = rtts.iter().sum::<f64>() / rtts.len() as f64;
                output.push(format!("rtt min/avg/max = {:.1}/{:.1}/{:.1} ms", min, avg, max));
            }
        }

        output.join("\n")
    }
}

/// Run detailed ping for the ping subcommand
#[allow(dead_code)] // Used in later phases
pub async fn ping_host_detailed(client: &Client, address: IpAddr, timeout: Duration, count: u32) -> DetailedPingResult {
    let identifier = PingIdentifier(random());
    let mut pinger = client.pinger(address, identifier).await;
    pinger.timeout(timeout);

    let payload = [0u8; 56];
    let mut results = Vec::with_capacity(count as usize);
    let mut packets_received = 0u32;

    for seq in 0..count {
        let result = match pinger.ping(PingSequence(seq as u16), &payload).await {
            Ok((_, rtt)) => {
                packets_received += 1;
                Ok(rtt)
            }
            Err(e) => Err(format_ping_error(&e, timeout)),
        };
        results.push((seq as u16, result));
    }

    DetailedPingResult {
        address,
        results,
        packets_sent: count,
        packets_received,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_ping_result_success_format() {
        let result = PingResult::success(
            "Test".to_string(),
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            Duration::from_millis(15),
        );
        assert!(result.success);
        assert!(result.format().contains("15.0ms"));
    }

    #[test]
    fn test_ping_result_failure_format() {
        let result = PingResult::failure(
            "Test".to_string(),
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            "timeout".to_string(),
        );
        assert!(!result.success);
        assert!(result.format().contains("timeout"));
    }

    #[test]
    fn test_detailed_ping_result_format() {
        let result = DetailedPingResult {
            address: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            results: vec![
                (0, Ok(Duration::from_millis(10))),
                (1, Ok(Duration::from_millis(12))),
                (2, Err("timeout".to_string())),
                (3, Ok(Duration::from_millis(11))),
            ],
            packets_sent: 4,
            packets_received: 3,
        };

        let output = result.format();
        assert!(output.contains("PING 8.8.8.8"));
        assert!(output.contains("4 packets transmitted, 3 received"));
        assert!(output.contains("25% packet loss"));
    }
}
