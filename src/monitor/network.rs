use crate::config::HostConfig;
use crate::dns;
use crate::monitor::app::{DnsCheckResult, PingCheckResult, Sample};
use crate::ping;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Spawn check tasks for all hosts
///
/// Returns a vector of join handles for the spawned tasks.
pub fn spawn_check_tasks(
    hosts: Vec<HostConfig>,
    interval: Duration,
    timeout: Duration,
    result_tx: mpsc::UnboundedSender<(String, Sample)>,
    cancel: CancellationToken,
) -> Vec<JoinHandle<()>> {
    let mut handles = Vec::with_capacity(hosts.len());

    // Create shared clients
    let dns_resolver = Arc::new(dns::create_resolver());
    let ping_client = Arc::new(ping::create_client().expect("Failed to create ping client"));

    for host in hosts {
        if !host.has_checks() {
            continue;
        }

        let resolver = dns_resolver.clone();
        let client = ping_client.clone();
        let tx = result_tx.clone();
        let cancel = cancel.clone();

        let handle = tokio::spawn(async move {
            check_host_loop(host, resolver, client, interval, timeout, tx, cancel).await;
        });

        handles.push(handle);
    }

    handles
}

/// Continuous check loop for a single host
async fn check_host_loop(
    host: HostConfig,
    resolver: Arc<hickory_resolver::TokioAsyncResolver>,
    client: Arc<surge_ping::Client>,
    interval: Duration,
    timeout: Duration,
    result_tx: mpsc::UnboundedSender<(String, Sample)>,
    cancel: CancellationToken,
) {
    // Do an immediate first check
    let sample = run_single_check(&host, &resolver, &client, timeout).await;
    let _ = result_tx.send((host.name.clone(), sample));

    // Then loop on interval
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            _ = tokio::time::sleep(interval) => {
                let sample = run_single_check(&host, &resolver, &client, timeout).await;
                let _ = result_tx.send((host.name.clone(), sample));
            }
        }
    }
}

/// Run a single check for a host
async fn run_single_check(
    host: &HostConfig,
    resolver: &hickory_resolver::TokioAsyncResolver,
    client: &surge_ping::Client,
    timeout: Duration,
) -> Sample {
    let timestamp = Instant::now();
    let mut dns_result = None;
    let mut ping_result = None;
    let mut resolved_ip: Option<IpAddr> = None;

    // Check if address is already an IP
    if let Ok(ip) = host.address.parse::<IpAddr>() {
        resolved_ip = Some(ip);
    }

    // DNS check (only if enabled and address is a hostname)
    if host.should_resolve_dns() {
        let (result, latency) = dns::resolve_dns_timed(resolver, &host.name, &host.address, true).await;

        if result.success && resolved_ip.is_none() {
            resolved_ip = result.addresses.first().copied();
        }

        dns_result = Some(DnsCheckResult {
            success: result.success,
            latency_ms: Some(latency.as_millis() as u64),
            addresses: result.addresses,
            error: result.error,
        });
    } else if resolved_ip.is_none() && host.ping {
        // Need to resolve for ping even if dns check not requested
        let result = dns::resolve_dns(resolver, &host.name, &host.address, false).await;
        if result.success {
            resolved_ip = result.addresses.first().copied();
        }
    }

    // Ping check
    if host.ping {
        if let Some(ip) = resolved_ip {
            let result = ping::ping_host(client, &host.name, ip, timeout, 1).await;
            ping_result = Some(PingCheckResult {
                success: result.success,
                rtt_ms: result.rtt.map(|d| d.as_millis() as u64),
                error: result.error,
            });
        } else {
            // Could not resolve hostname for ping
            ping_result = Some(PingCheckResult {
                success: false,
                rtt_ms: None,
                error: Some("could not resolve hostname".to_string()),
            });
        }
    }

    Sample {
        timestamp,
        dns_result,
        ping_result,
    }
}
