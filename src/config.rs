use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Timeout for ping/dns operations in milliseconds
    pub timeout_ms: u64,
    /// Number of retry attempts
    pub retry_count: u32,
    /// List of hosts to check
    pub hosts: Vec<HostConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timeout_ms: 1000,
            retry_count: 3,
            hosts: vec![],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HostConfig {
    /// Display name for the host
    pub name: String,
    /// IP address or hostname
    pub address: String,
    /// Whether to perform ping check
    #[serde(default)]
    pub ping: bool,
    /// Whether to perform DNS resolution (only valid for hostnames, not IPs)
    #[serde(default)]
    pub dns: bool,
}

impl HostConfig {
    /// Check if the address is an IP address (not a hostname)
    #[allow(dead_code)] // Used in later phases
    pub fn is_ip_address(&self) -> bool {
        self.address.parse::<IpAddr>().is_ok()
    }

    /// Check if this host has any checks enabled
    #[allow(dead_code)] // Used in later phases
    pub fn has_checks(&self) -> bool {
        self.ping || self.dns
    }

    /// Check if DNS resolution should be performed
    /// Returns false if address is already an IP (DNS not needed)
    #[allow(dead_code)] // Used in later phases
    pub fn should_resolve_dns(&self) -> bool {
        self.dns && !self.is_ip_address()
    }
}

impl Config {
    /// Load configuration with fallback chain
    pub fn load(config_path: Option<&PathBuf>) -> Result<Self> {
        // If explicit config path provided, try to load it
        if let Some(path) = config_path {
            return Self::load_from_file(path).context(format!("Failed to load config from {}", path.display()));
        }

        // Try primary location: ~/.config/cxn/cxn.yml
        if let Some(config_dir) = dirs::config_dir() {
            let project_name = env!("CARGO_PKG_NAME");
            let primary_config = config_dir.join(project_name).join(format!("{}.yml", project_name));
            if primary_config.exists() {
                match Self::load_from_file(&primary_config) {
                    Ok(config) => return Ok(config),
                    Err(e) => {
                        log::warn!("Failed to load config from {}: {}", primary_config.display(), e);
                    }
                }
            }
        }

        // Try fallback location: ./cxn.yml
        let project_name = env!("CARGO_PKG_NAME");
        let fallback_config = PathBuf::from(format!("{}.yml", project_name));
        if fallback_config.exists() {
            match Self::load_from_file(&fallback_config) {
                Ok(config) => return Ok(config),
                Err(e) => {
                    log::warn!("Failed to load config from {}: {}", fallback_config.display(), e);
                }
            }
        }

        // No config file found, use defaults
        log::info!("No config file found, using defaults");
        Ok(Self::default())
    }

    fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).context("Failed to read config file")?;

        let config: Self = serde_yaml::from_str(&content).context("Failed to parse config file")?;

        log::info!("Loaded config from: {}", path.as_ref().display());
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.timeout_ms, 1000);
        assert_eq!(config.retry_count, 3);
        assert!(config.hosts.is_empty());
    }

    #[test]
    fn test_host_config_is_ip_address() {
        let ip_host = HostConfig {
            name: "Test".to_string(),
            address: "8.8.8.8".to_string(),
            ping: true,
            dns: false,
        };
        assert!(ip_host.is_ip_address());

        let hostname_host = HostConfig {
            name: "Test".to_string(),
            address: "google.com".to_string(),
            ping: true,
            dns: true,
        };
        assert!(!hostname_host.is_ip_address());
    }

    #[test]
    fn test_host_config_should_resolve_dns() {
        // IP address with dns: true -> should NOT resolve (already an IP)
        let ip_host = HostConfig {
            name: "Test".to_string(),
            address: "8.8.8.8".to_string(),
            ping: true,
            dns: true,
        };
        assert!(!ip_host.should_resolve_dns());

        // Hostname with dns: true -> should resolve
        let hostname_host = HostConfig {
            name: "Test".to_string(),
            address: "google.com".to_string(),
            ping: true,
            dns: true,
        };
        assert!(hostname_host.should_resolve_dns());

        // Hostname with dns: false -> should NOT resolve
        let hostname_no_dns = HostConfig {
            name: "Test".to_string(),
            address: "google.com".to_string(),
            ping: true,
            dns: false,
        };
        assert!(!hostname_no_dns.should_resolve_dns());
    }

    #[test]
    fn test_config_parse_yaml() {
        let yaml = r#"
timeout_ms: 2000
retry_count: 5
hosts:
  - name: "Google DNS"
    address: "8.8.8.8"
    ping: true
    dns: false
  - name: "GitHub"
    address: "github.com"
    ping: true
    dns: true
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.timeout_ms, 2000);
        assert_eq!(config.retry_count, 5);
        assert_eq!(config.hosts.len(), 2);
        assert_eq!(config.hosts[0].name, "Google DNS");
        assert!(config.hosts[0].ping);
        assert!(!config.hosts[0].dns);
    }
}
