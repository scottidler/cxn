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
    dns_recheck: u32,
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
            check_host_loop(CheckHostParams {
                host,
                resolver,
                client,
                interval,
                timeout,
                dns_recheck,
                result_tx: tx,
                cancel,
            })
            .await;
        });

        handles.push(handle);
    }

    handles
}

struct CheckHostParams {
    host: HostConfig,
    resolver: Arc<hickory_resolver::TokioAsyncResolver>,
    client: Arc<surge_ping::Client>,
    interval: Duration,
    timeout: Duration,
    dns_recheck: u32,
    result_tx: mpsc::UnboundedSender<(String, Sample)>,
    cancel: CancellationToken,
}

/// Continuous check loop for a single host
/// Uses DNS caching for faster ping-only iterations
async fn check_host_loop(params: CheckHostParams) {
    let CheckHostParams {
        host,
        resolver,
        client,
        interval,
        timeout,
        dns_recheck,
        result_tx,
        cancel,
    } = params;
    // Cache for resolved IP address
    let mut cached_ip: Option<IpAddr> = None;
    let mut checks_since_dns: u32 = 0;

    // Check if address is already an IP (no DNS needed)
    if let Ok(ip) = host.address.parse::<IpAddr>() {
        cached_ip = Some(ip);
    }

    // Do an immediate first check (always does DNS if needed)
    let (sample, resolved_ip) = run_single_check_with_cache(&host, &resolver, &client, timeout, None, true).await;
    if resolved_ip.is_some() {
        cached_ip = resolved_ip;
    }
    let _ = result_tx.send((host.name.clone(), sample));

    // Then loop on interval
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            _ = tokio::time::sleep(interval) => {
                checks_since_dns += 1;

                // Re-resolve DNS periodically or if we don't have a cached IP
                // dns_recheck of 0 means re-resolve every time
                let should_resolve_dns = host.should_resolve_dns() &&
                    (cached_ip.is_none() || dns_recheck == 0 || checks_since_dns >= dns_recheck);

                let (sample, resolved_ip) = run_single_check_with_cache(
                    &host, &resolver, &client, timeout, cached_ip, should_resolve_dns
                ).await;

                // Update cache
                if let Some(ip) = resolved_ip {
                    cached_ip = Some(ip);
                    checks_since_dns = 0;
                }

                // If ping failed and we have a cached IP, might be stale - force DNS re-resolve next time
                if dns_recheck > 0 && cached_ip.is_some() && sample.ping_result.as_ref().is_some_and(|p| !p.success) {
                    checks_since_dns = dns_recheck; // Force re-resolve next iteration
                }

                let _ = result_tx.send((host.name.clone(), sample));
            }
        }
    }
}

/// Run a single check for a host with optional DNS caching
/// Returns (Sample, Option<IpAddr>) where the IP is the newly resolved address (if any)
async fn run_single_check_with_cache(
    host: &HostConfig,
    resolver: &hickory_resolver::TokioAsyncResolver,
    client: &surge_ping::Client,
    timeout: Duration,
    cached_ip: Option<IpAddr>,
    do_dns_check: bool,
) -> (Sample, Option<IpAddr>) {
    let timestamp = Instant::now();
    let mut dns_result = None;
    let mut ping_result = None;
    let mut resolved_ip: Option<IpAddr> = cached_ip;
    let mut new_ip: Option<IpAddr> = None;

    // Check if address is already an IP
    if let Ok(ip) = host.address.parse::<IpAddr>() {
        resolved_ip = Some(ip);
    }

    // DNS check (only if requested)
    if do_dns_check && host.should_resolve_dns() {
        let (result, latency) = dns::resolve_dns_timed(resolver, &host.name, &host.address, true).await;

        if result.success {
            new_ip = result.addresses.first().copied();
            if resolved_ip.is_none() {
                resolved_ip = new_ip;
            }
        }

        dns_result = Some(DnsCheckResult {
            success: result.success,
            latency_ms: Some(latency.as_millis() as u64),
            addresses: result.addresses,
            error: result.error,
        });
    } else if resolved_ip.is_none() && host.ping {
        // Need to resolve for ping even if dns check not requested (first time only)
        let result = dns::resolve_dns(resolver, &host.name, &host.address, false).await;
        if result.success {
            new_ip = result.addresses.first().copied();
            resolved_ip = new_ip;
        }
    }

    // Ping check (fast path using cached IP)
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

    (
        Sample {
            timestamp,
            dns_result,
            ping_result,
        },
        new_ip,
    )
}
