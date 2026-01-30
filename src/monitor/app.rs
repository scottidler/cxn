use crate::config::Config;
use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::VecDeque;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Maximum number of errors to keep in the error log
const MAX_ERROR_LOG: usize = 100;

/// A fixed-capacity ring buffer for storing samples
#[derive(Debug, Clone)]
pub struct RingBuffer<T> {
    data: VecDeque<T>,
    capacity: usize,
}

impl<T> RingBuffer<T> {
    /// Create a new ring buffer with the given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            data: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push an item to the buffer, removing the oldest if at capacity
    pub fn push(&mut self, item: T) {
        if self.data.len() >= self.capacity {
            self.data.pop_front();
        }
        self.data.push_back(item);
    }

    /// Get an iterator over the buffer items (oldest to newest)
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    /// Get the number of items in the buffer
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the buffer is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Result of a DNS check
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DnsCheckResult {
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub addresses: Vec<IpAddr>,
    pub error: Option<String>,
}

/// Result of a ping check
#[derive(Debug, Clone)]
pub struct PingCheckResult {
    pub success: bool,
    pub rtt_ms: Option<u64>,
    pub error: Option<String>,
}

/// A single sample from a check cycle
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Sample {
    pub timestamp: Instant,
    pub dns_result: Option<DnsCheckResult>,
    pub ping_result: Option<PingCheckResult>,
}

impl Sample {
    /// Check if this sample represents a success
    pub fn is_success(&self) -> bool {
        let dns_ok = self.dns_result.as_ref().is_none_or(|r| r.success);
        let ping_ok = self.ping_result.as_ref().is_none_or(|r| r.success);
        dns_ok && ping_ok
    }
}

/// State for a single host being monitored
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HostState {
    pub name: String,
    pub address: String,
    pub last_sample: Option<Sample>,
    pub history: RingBuffer<Sample>,
    pub error_count: u32,
    pub success_count: u32,
    pub ping_enabled: bool,
    pub dns_enabled: bool,
}

impl HostState {
    /// Create a new host state
    pub fn new(name: String, address: String, ping_enabled: bool, dns_enabled: bool, history_size: usize) -> Self {
        Self {
            name,
            address,
            last_sample: None,
            history: RingBuffer::new(history_size),
            error_count: 0,
            success_count: 0,
            ping_enabled,
            dns_enabled,
        }
    }

    /// Record a new sample for this host
    pub fn record_sample(&mut self, sample: Sample) {
        if sample.is_success() {
            self.success_count += 1;
        } else {
            self.error_count += 1;
        }
        self.last_sample = Some(sample.clone());
        self.history.push(sample);
    }

    /// Get the current status symbol
    pub fn status_symbol(&self) -> &'static str {
        match &self.last_sample {
            None => "?",
            Some(s) if s.is_success() => "✓",
            Some(_) => "✗",
        }
    }

    /// Get ping latency as a formatted string
    pub fn ping_display(&self) -> String {
        if !self.ping_enabled {
            return "-".to_string();
        }
        match &self.last_sample {
            None => "-".to_string(),
            Some(s) => match &s.ping_result {
                None => "-".to_string(),
                Some(p) if p.success => p
                    .rtt_ms
                    .map(|ms| format!("{}ms", ms))
                    .unwrap_or_else(|| "ok".to_string()),
                Some(_) => "FAIL".to_string(),
            },
        }
    }

    /// Get DNS latency as a formatted string
    pub fn dns_display(&self) -> String {
        if !self.dns_enabled {
            return "-".to_string();
        }
        match &self.last_sample {
            None => "-".to_string(),
            Some(s) => match &s.dns_result {
                None => "-".to_string(),
                Some(d) if d.success => d
                    .latency_ms
                    .map(|ms| format!("{}ms", ms))
                    .unwrap_or_else(|| "ok".to_string()),
                Some(_) => "FAIL".to_string(),
            },
        }
    }

    /// Get sparkline data (ping latencies, with failures as max value)
    pub fn sparkline_data(&self) -> Vec<u64> {
        // Find max latency for scaling failures
        let max_latency = self
            .history
            .iter()
            .filter_map(|s| s.ping_result.as_ref())
            .filter_map(|p| p.rtt_ms)
            .max()
            .unwrap_or(100);

        // Use a value higher than max for failures to make them stand out
        let failure_value = max_latency.saturating_mul(2).max(100);

        self.history
            .iter()
            .map(|s| {
                if !s.is_success() {
                    failure_value
                } else {
                    s.ping_result.as_ref().and_then(|p| p.rtt_ms).unwrap_or(1)
                }
            })
            .collect()
    }
}

/// Type of error that occurred
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ErrorType {
    DnsTimeout,
    DnsError,
    PingTimeout,
    PingError,
}

impl ErrorType {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorType::DnsTimeout => "DNS timeout",
            ErrorType::DnsError => "DNS error",
            ErrorType::PingTimeout => "Ping timeout",
            ErrorType::PingError => "Ping error",
        }
    }
}

/// An entry in the error log
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ErrorEntry {
    pub timestamp: DateTime<Local>,
    pub host: String,
    pub error_type: ErrorType,
    pub message: String,
}

impl ErrorEntry {
    /// Format the error entry for display
    #[allow(dead_code)]
    pub fn format(&self) -> String {
        format!(
            "{} {:12} {}",
            self.timestamp.format("%H:%M:%S"),
            self.host,
            self.message
        )
    }
}

/// Global statistics for the monitor
#[derive(Debug, Clone)]
pub struct GlobalStats {
    pub total_checks: u64,
    pub total_errors: u64,
    pub start_time: Instant,
}

impl GlobalStats {
    /// Create new global stats
    pub fn new() -> Self {
        Self {
            total_checks: 0,
            total_errors: 0,
            start_time: Instant::now(),
        }
    }

    /// Get uptime as a formatted string
    pub fn uptime_display(&self) -> String {
        let elapsed = self.start_time.elapsed();
        let hours = elapsed.as_secs() / 3600;
        let minutes = (elapsed.as_secs() % 3600) / 60;

        if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}m", minutes)
        }
    }
}

impl Default for GlobalStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Main application state
pub struct App {
    pub hosts: Vec<HostState>,
    pub error_log: VecDeque<ErrorEntry>,
    pub stats: GlobalStats,
    pub selected_host: usize,
    pub should_quit: bool,
    pub interval: Duration,
}

impl App {
    /// Create a new App from config
    pub fn new(config: &Config, history_size: usize) -> Self {
        let hosts = config
            .hosts()
            .into_iter()
            .map(|h| {
                let dns_enabled = h.should_resolve_dns();
                HostState::new(h.name, h.address, h.ping, dns_enabled, history_size)
            })
            .collect();

        Self {
            hosts,
            error_log: VecDeque::with_capacity(MAX_ERROR_LOG),
            stats: GlobalStats::new(),
            selected_host: 0,
            should_quit: false,
            interval: Duration::from_secs(config.interval),
        }
    }

    /// Record a sample for a host
    pub fn record_sample(&mut self, host_name: &str, sample: Sample) {
        self.stats.total_checks += 1;

        // Check for errors and collect error entries first
        let mut errors_to_add = Vec::new();

        if !sample.is_success() {
            self.stats.total_errors += 1;

            // Determine error type and message for DNS
            if let Some(ref dns) = sample.dns_result
                && !dns.success
            {
                let error_type = if dns.error.as_ref().is_some_and(|e| e.contains("timeout")) {
                    ErrorType::DnsTimeout
                } else {
                    ErrorType::DnsError
                };
                errors_to_add.push((
                    host_name.to_string(),
                    error_type,
                    dns.error.clone().unwrap_or_else(|| "DNS failed".to_string()),
                ));
            }

            // Determine error type and message for ping
            if let Some(ref ping) = sample.ping_result
                && !ping.success
            {
                let error_type = if ping.error.as_ref().is_some_and(|e| e.contains("timeout")) {
                    ErrorType::PingTimeout
                } else {
                    ErrorType::PingError
                };
                errors_to_add.push((
                    host_name.to_string(),
                    error_type,
                    ping.error.clone().unwrap_or_else(|| "Ping failed".to_string()),
                ));
            }
        }

        // Add errors to log
        for (host, error_type, message) in errors_to_add {
            self.add_error(host, error_type, message);
        }

        // Find the host and record the sample
        if let Some(host) = self.hosts.iter_mut().find(|h| h.name == host_name) {
            host.record_sample(sample);
        }
    }

    /// Add an error to the error log
    fn add_error(&mut self, host: String, error_type: ErrorType, message: String) {
        if self.error_log.len() >= MAX_ERROR_LOG {
            self.error_log.pop_front();
        }
        self.error_log.push_back(ErrorEntry {
            timestamp: Local::now(),
            host,
            error_type,
            message,
        });
    }

    /// Handle a key event
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('c') => {
                // Clear error log
                self.error_log.clear();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_host > 0 {
                    self.selected_host -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_host + 1 < self.hosts.len() {
                    self.selected_host += 1;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer() {
        let mut buf: RingBuffer<i32> = RingBuffer::new(3);
        assert!(buf.is_empty());

        buf.push(1);
        buf.push(2);
        buf.push(3);
        assert_eq!(buf.len(), 3);

        buf.push(4); // Should evict 1
        assert_eq!(buf.len(), 3);
        let items: Vec<_> = buf.iter().cloned().collect();
        assert_eq!(items, vec![2, 3, 4]);
    }

    #[test]
    fn test_global_stats_uptime() {
        let stats = GlobalStats::new();
        // Just verify it doesn't panic
        let _ = stats.uptime_display();
    }
}
