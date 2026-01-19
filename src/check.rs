use crate::config::{Config, HostConfig};
use crate::dns::{self, DnsResult};
use crate::ping::{self, PingResult};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use surge_ping::Client as PingClient;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Maximum number of concurrent checks
const MAX_CONCURRENT_CHECKS: usize = 20;

/// Result of checking a single host
#[derive(Debug)]
pub struct CheckResult {
    /// Display name from config
    pub name: String,
    /// Original address from config
    pub address: String,
    /// DNS resolution result (if performed)
    pub dns: Option<DnsResult>,
    /// Ping result (if performed)
    pub ping: Option<PingResult>,
}

impl CheckResult {
    /// Check if all performed checks were successful
    pub fn is_success(&self) -> bool {
        let dns_ok = self.dns.as_ref().is_none_or(|r| r.success);
        let ping_ok = self.ping.as_ref().is_none_or(|r| r.success);
        dns_ok && ping_ok
    }
}

/// Run all configured host checks
///
/// If `parallel` is true, runs checks concurrently with bounded concurrency.
/// Otherwise, runs checks sequentially.
pub async fn run_all_checks(
    config: &Config,
    ping_client: Arc<PingClient>,
    dns_resolver: Arc<hickory_resolver::TokioAsyncResolver>,
    parallel: bool,
) -> Vec<CheckResult> {
    let timeout = Duration::from_millis(config.timeout);

    if parallel {
        run_parallel_checks(config, ping_client, dns_resolver, timeout).await
    } else {
        run_sequential_checks(config, ping_client, dns_resolver, timeout).await
    }
}

/// Run checks in parallel with bounded concurrency
async fn run_parallel_checks(
    config: &Config,
    ping_client: Arc<PingClient>,
    dns_resolver: Arc<hickory_resolver::TokioAsyncResolver>,
    timeout: Duration,
) -> Vec<CheckResult> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CHECKS));
    let mut join_set = JoinSet::new();

    let hosts = config.hosts();

    for (idx, host) in hosts.iter().enumerate() {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let ping_client = ping_client.clone();
        let dns_resolver = dns_resolver.clone();
        let host = host.clone();

        join_set.spawn(async move {
            let result = check_host(&host, &ping_client, &dns_resolver, timeout).await;
            drop(permit);
            (idx, result)
        });
    }

    // Collect results and sort by original order
    let mut results: Vec<(usize, CheckResult)> = Vec::with_capacity(hosts.len());
    while let Some(Ok((idx, result))) = join_set.join_next().await {
        results.push((idx, result));
    }
    results.sort_by_key(|(idx, _)| *idx);
    results.into_iter().map(|(_, r)| r).collect()
}

/// Run checks sequentially
async fn run_sequential_checks(
    config: &Config,
    ping_client: Arc<PingClient>,
    dns_resolver: Arc<hickory_resolver::TokioAsyncResolver>,
    timeout: Duration,
) -> Vec<CheckResult> {
    let hosts = config.hosts();
    let mut results = Vec::with_capacity(hosts.len());

    for host in &hosts {
        let result = check_host(host, &ping_client, &dns_resolver, timeout).await;
        results.push(result);
    }

    results
}

/// Check a single host
async fn check_host(
    host: &HostConfig,
    ping_client: &PingClient,
    dns_resolver: &hickory_resolver::TokioAsyncResolver,
    timeout: Duration,
) -> CheckResult {
    let mut dns_result = None;
    let mut ping_result = None;
    let mut resolved_ip: Option<IpAddr> = None;

    // Check if address is already an IP
    if let Ok(ip) = host.address.parse::<IpAddr>() {
        resolved_ip = Some(ip);
    }

    // DNS check (only if enabled and address is a hostname)
    if host.should_resolve_dns() {
        let result = dns::resolve_dns(dns_resolver, &host.name, &host.address, true).await;
        if result.success && resolved_ip.is_none() {
            resolved_ip = result.addresses.first().copied();
        }
        dns_result = Some(result);
    } else if resolved_ip.is_none() && host.ping {
        // Need to resolve for ping even if dns check not requested
        let result = dns::resolve_dns(dns_resolver, &host.name, &host.address, false).await;
        if result.success {
            resolved_ip = result.addresses.first().copied();
        }
    }

    // Ping check
    if host.ping {
        if let Some(ip) = resolved_ip {
            let result = ping::ping_host(ping_client, &host.name, ip, timeout, 1).await;
            ping_result = Some(result);
        } else {
            // Could not resolve hostname for ping
            ping_result = Some(PingResult::failure(
                host.name.clone(),
                "0.0.0.0".parse().unwrap(),
                "could not resolve hostname".to_string(),
            ));
        }
    }

    CheckResult {
        name: host.name.clone(),
        address: host.address.clone(),
        dns: dns_result,
        ping: ping_result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_result_success() {
        let result = CheckResult {
            name: "Test".to_string(),
            address: "8.8.8.8".to_string(),
            dns: None,
            ping: Some(PingResult::success(
                "Test".to_string(),
                "8.8.8.8".parse().unwrap(),
                Duration::from_millis(10),
            )),
        };
        assert!(result.is_success());
    }

    #[test]
    fn test_check_result_ping_failure() {
        let result = CheckResult {
            name: "Test".to_string(),
            address: "8.8.8.8".to_string(),
            dns: None,
            ping: Some(PingResult::failure(
                "Test".to_string(),
                "8.8.8.8".parse().unwrap(),
                "timeout".to_string(),
            )),
        };
        assert!(!result.is_success());
    }

    #[test]
    fn test_check_result_dns_failure() {
        let result = CheckResult {
            name: "Test".to_string(),
            address: "bad.invalid".to_string(),
            dns: Some(DnsResult::failure(
                "Test".to_string(),
                "bad.invalid".to_string(),
                "no such host".to_string(),
            )),
            ping: None,
        };
        assert!(!result.is_success());
    }

    #[test]
    fn test_check_result_no_checks() {
        let result = CheckResult {
            name: "Test".to_string(),
            address: "8.8.8.8".to_string(),
            dns: None,
            ping: None,
        };
        // No checks means vacuously successful
        assert!(result.is_success());
    }
}
