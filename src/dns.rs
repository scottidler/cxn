use colored::*;
use hickory_resolver::TokioAsyncResolver;
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use std::net::IpAddr;

/// Result of a DNS resolution operation
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used in later phases
pub struct DnsResult {
    /// Display name from config
    pub name: String,
    /// The hostname that was resolved
    pub hostname: String,
    /// Whether the resolution was successful
    pub success: bool,
    /// Resolved IP addresses
    pub addresses: Vec<IpAddr>,
    /// Error message if failed
    pub error: Option<String>,
}

#[allow(dead_code)] // Used in later phases
impl DnsResult {
    /// Create a successful DNS result
    pub fn success(name: String, hostname: String, addresses: Vec<IpAddr>) -> Self {
        Self {
            name,
            hostname,
            success: true,
            addresses,
            error: None,
        }
    }

    /// Create a failed DNS result
    pub fn failure(name: String, hostname: String, error: String) -> Self {
        Self {
            name,
            hostname,
            success: false,
            addresses: vec![],
            error: Some(error),
        }
    }

    /// Format the result for display
    pub fn format(&self) -> String {
        if self.success {
            let addrs = if self.addresses.is_empty() {
                "(none)".to_string()
            } else {
                self.addresses
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!("  {} dns:  {}", "✓".green(), addrs)
        } else {
            let err_str = self.error.as_deref().unwrap_or("unknown error");
            format!("  {} dns:  {}", "✗".red(), err_str)
        }
    }
}

/// Create a new DNS resolver using system configuration
#[allow(dead_code)] // Used in later phases
pub fn create_resolver() -> TokioAsyncResolver {
    TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default())
}

/// Resolve DNS for a hostname
///
/// Performs A and optionally AAAA lookups for the given hostname.
#[allow(dead_code)] // Used in later phases
pub async fn resolve_dns(resolver: &TokioAsyncResolver, name: &str, hostname: &str, include_ipv6: bool) -> DnsResult {
    let mut addresses = Vec::new();

    // Try IPv4 lookup
    match resolver.lookup_ip(hostname).await {
        Ok(lookup) => {
            for ip in lookup.iter() {
                if ip.is_ipv4() || include_ipv6 {
                    addresses.push(ip);
                }
            }
        }
        Err(e) => {
            return DnsResult::failure(name.to_string(), hostname.to_string(), format_dns_error(&e));
        }
    }

    if addresses.is_empty() {
        DnsResult::failure(name.to_string(), hostname.to_string(), "no addresses found".to_string())
    } else {
        DnsResult::success(name.to_string(), hostname.to_string(), addresses)
    }
}

/// Format a DNS error into a user-friendly message
#[allow(dead_code)] // Used in later phases
fn format_dns_error(error: &hickory_resolver::error::ResolveError) -> String {
    use hickory_resolver::error::ResolveErrorKind;

    match error.kind() {
        ResolveErrorKind::NoRecordsFound { .. } => "no such host".to_string(),
        ResolveErrorKind::Timeout => "timeout".to_string(),
        ResolveErrorKind::Io(io_err) => format!("io error: {}", io_err),
        _ => format!("{}", error),
    }
}

/// Detailed DNS result for the `cxn dns` subcommand
#[allow(dead_code)] // Used in later phases
pub struct DetailedDnsResult {
    pub hostname: String,
    pub ipv4_addresses: Vec<IpAddr>,
    pub ipv6_addresses: Vec<IpAddr>,
    pub error: Option<String>,
}

#[allow(dead_code)] // Used in later phases
impl DetailedDnsResult {
    /// Format detailed output for the dns subcommand
    pub fn format(&self) -> String {
        let mut output = vec![self.hostname.clone()];

        if let Some(ref err) = self.error {
            output.push(format!("  {}: {}", "Error".red(), err));
            return output.join("\n");
        }

        // Format IPv4 addresses
        if self.ipv4_addresses.is_empty() {
            output.push(format!("  A:    {}", "(none)".dimmed()));
        } else {
            for (i, addr) in self.ipv4_addresses.iter().enumerate() {
                if i == 0 {
                    output.push(format!("  A:    {}", addr));
                } else {
                    output.push(format!("        {}", addr));
                }
            }
        }

        // Format IPv6 addresses
        if self.ipv6_addresses.is_empty() {
            output.push(format!("  AAAA: {}", "(none)".dimmed()));
        } else {
            for (i, addr) in self.ipv6_addresses.iter().enumerate() {
                if i == 0 {
                    output.push(format!("  AAAA: {}", addr));
                } else {
                    output.push(format!("        {}", addr));
                }
            }
        }

        output.join("\n")
    }
}

/// Run detailed DNS resolution for the dns subcommand
#[allow(dead_code)] // Used in later phases
pub async fn resolve_dns_detailed(
    resolver: &TokioAsyncResolver,
    hostname: &str,
    include_ipv6: bool,
) -> DetailedDnsResult {
    let mut ipv4_addresses = Vec::new();
    let mut ipv6_addresses = Vec::new();

    match resolver.lookup_ip(hostname).await {
        Ok(lookup) => {
            for ip in lookup.iter() {
                if ip.is_ipv4() {
                    ipv4_addresses.push(ip);
                } else if include_ipv6 {
                    ipv6_addresses.push(ip);
                }
            }

            DetailedDnsResult {
                hostname: hostname.to_string(),
                ipv4_addresses,
                ipv6_addresses,
                error: None,
            }
        }
        Err(e) => DetailedDnsResult {
            hostname: hostname.to_string(),
            ipv4_addresses: vec![],
            ipv6_addresses: vec![],
            error: Some(format_dns_error(&e)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_dns_result_success_format() {
        let result = DnsResult::success(
            "Test".to_string(),
            "example.com".to_string(),
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))],
        );
        assert!(result.success);
        let formatted = result.format();
        assert!(formatted.contains("93.184.216.34"));
    }

    #[test]
    fn test_dns_result_failure_format() {
        let result = DnsResult::failure(
            "Test".to_string(),
            "bad.invalid".to_string(),
            "no such host".to_string(),
        );
        assert!(!result.success);
        let formatted = result.format();
        assert!(formatted.contains("no such host"));
    }

    #[test]
    fn test_detailed_dns_result_format() {
        let result = DetailedDnsResult {
            hostname: "example.com".to_string(),
            ipv4_addresses: vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))],
            ipv6_addresses: vec![],
            error: None,
        };

        let output = result.format();
        assert!(output.contains("example.com"));
        assert!(output.contains("93.184.216.34"));
        assert!(output.contains("A:"));
    }

    #[test]
    fn test_detailed_dns_result_error_format() {
        let result = DetailedDnsResult {
            hostname: "bad.invalid".to_string(),
            ipv4_addresses: vec![],
            ipv6_addresses: vec![],
            error: Some("no such host".to_string()),
        };

        let output = result.format();
        assert!(output.contains("bad.invalid"));
        assert!(output.contains("no such host"));
    }
}
