# Design Document: cxn - Connection Testing CLI

**Author:** Scott A. Idler
**Date:** 2026-01-18
**Status:** Ready for Review
**Review Passes:** 5/5

## Summary

`cxn` is a Rust CLI tool that provides quick ping and DNS connectivity checks against a configurable list of hosts. It reads host definitions from `~/.config/cxn/cxn.yml` and reports connection status with colored output for easy visual scanning.

## Problem Statement

### Background

When troubleshooting network connectivity issues, engineers frequently need to verify basic reachability (ICMP ping) and DNS resolution. This typically involves running multiple manual commands (`ping`, `dig`, `nslookup`) against various hosts. A unified tool that checks multiple hosts with a single command saves time and provides consistent output.

### Problem

There is no simple, configurable CLI tool that:
1. Checks both ping and DNS connectivity in one command
2. Uses a persistent configuration file for frequently-tested hosts
3. Provides clear, colored output for quick visual assessment
4. Supports parallel checking for efficiency

### Goals

- Provide ICMP ping checks against configured hosts
- Provide DNS resolution checks against configured hostnames
- Load host configuration from YAML file (`~/.config/cxn/cxn.yml`)
- Display results with colored output (green=success, red=failure)
- Support parallel host checking for efficiency
- Provide individual `ping` and `dns` subcommands for ad-hoc testing

### Non-Goals

- HTTP/HTTPS connectivity testing (out of scope for v1)
- Port scanning or TCP connection testing
- Continuous monitoring or alerting
- Network path tracing (traceroute)
- Latency statistics or historical data

## Proposed Solution

### Overview

Transform the existing `cxn` scaffold into a connection testing tool with:
1. New `Config` struct with host definitions
2. `ping` module wrapping `surge-ping` for ICMP checks
3. `dns` module wrapping `hickory-resolver` for DNS resolution
4. Subcommands: `ping`, `dns`, and `check` (runs all configured checks)
5. Async execution via Tokio for parallel host checking

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        main.rs                              │
│  - #[tokio::main] entry point                               │
│  - CLI parsing and command dispatch                         │
│  - Config loading                                           │
│  - Client/Resolver initialization                           │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
┌───────────────┐    ┌───────────────┐    ┌───────────────┐
│   cli.rs      │    │  config.rs    │    │   check.rs    │
│               │    │               │    │               │
│ - Cli struct  │    │ - Config      │    │ - Checker     │
│ - Commands    │    │ - HostConfig  │    │ - run_checks  │
│   enum        │    │ - validate()  │    │ - JoinSet     │
└───────────────┘    └───────────────┘    └───────────────┘
                                                  │
                          ┌───────────────────────┼───────────┐
                          ▼                                   ▼
                   ┌───────────────┐                  ┌───────────────┐
                   │   ping.rs     │                  │    dns.rs     │
                   │               │                  │               │
                   │ - PingClient  │                  │ - DnsClient   │
                   │ - ping_host   │                  │ - resolve     │
                   │ - PingResult  │                  │ - DnsResult   │
                   └───────────────┘                  └───────────────┘
                                    │
                                    ▼
                          ┌───────────────┐
                          │  output.rs    │
                          │               │
                          │ - print_result│
                          │ - print_summary│
                          │ - colored fmt │
                          └───────────────┘
```

### Component Lifecycle

**Client Initialization (in main.rs):**

```rust
// Create shared clients once at startup
let ping_client = Arc::new(
    surge_ping::Client::new(&surge_ping::Config::default())
        .context("Failed to create ping client")?
);

let dns_resolver = Arc::new(
    hickory_resolver::Resolver::tokio_from_system_conf()
        .context("Failed to create DNS resolver")?
);
```

**Concurrency Control:**

```rust
// In check.rs - bounded concurrency for large host lists
const MAX_CONCURRENT_CHECKS: usize = 20;

pub async fn run_all_checks(
    config: &Config,
    ping_client: Arc<surge_ping::Client>,
    dns_resolver: Arc<Resolver>,
) -> Vec<CheckResult> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CHECKS));
    let mut join_set = JoinSet::new();

    for host in &config.hosts {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let ping_client = ping_client.clone();
        let dns_resolver = dns_resolver.clone();
        let host = host.clone();
        let timeout = Duration::from_millis(config.timeout_ms);

        join_set.spawn(async move {
            let result = check_host(&host, &ping_client, &dns_resolver, timeout).await;
            drop(permit); // Release semaphore
            result
        });
    }

    // Collect results preserving order
    let mut results = Vec::with_capacity(config.hosts.len());
    while let Some(result) = join_set.join_next().await {
        if let Ok(check_result) = result {
            results.push(check_result);
        }
    }
    results
}
```

### Data Model

**Config Structure:**

```rust
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Timeout for ping/dns operations in milliseconds
    pub timeout_ms: u64,
    /// Number of retry attempts
    pub retry_count: u32,
    /// List of hosts to check
    pub hosts: Vec<HostConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
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
```

**Validation Rules:**
- If `address` is a valid IP address and `dns: true`, the DNS check is skipped (DNS resolution only applies to hostnames)
- If `address` is a hostname and `ping: true`, DNS resolution is performed first to obtain the IP for pinging
- At least one of `ping` or `dns` should be `true` for meaningful results

**Default Implementation:**

```rust
impl Default for Config {
    fn default() -> Self {
        Self {
            timeout_ms: 1000,
            retry_count: 3,
            hosts: vec![],
        }
    }
}
```

**Result Types:**

```rust
pub struct PingResult {
    pub name: String,        // Display name from config
    pub address: IpAddr,     // Resolved or provided IP
    pub success: bool,
    pub rtt: Option<Duration>,
    pub error: Option<String>,
}

pub struct DnsResult {
    pub name: String,        // Display name from config
    pub hostname: String,    // The hostname that was resolved
    pub success: bool,
    pub addresses: Vec<IpAddr>,
    pub error: Option<String>,
}

pub struct CheckResult {
    pub name: String,        // Display name from config
    pub address: String,     // Original address from config
    pub ping: Option<PingResult>,
    pub dns: Option<DnsResult>,
}
```

### API Design

**CLI Interface:**

```
cxn [OPTIONS] [COMMAND]

Commands:
  ping   Ping a host to check reachability
  dns    Resolve DNS for a hostname
  check  Check connectivity for all configured hosts (default)

Options:
  -c, --config <PATH>  Path to config file
  -v, --verbose        Enable verbose output
  -h, --help           Print help
  -V, --version        Print version

cxn ping <HOST> [OPTIONS]
  -c, --count <N>      Number of pings (default: 4)
  -t, --timeout <MS>   Timeout in milliseconds (default: 1000)

cxn dns <HOSTNAME> [OPTIONS]
  -6, --ipv6           Include IPv6 addresses

cxn check [OPTIONS]
  -p, --parallel       Run checks in parallel (default: true)
```

**Module APIs:**

```rust
// ping.rs
pub async fn ping_host(
    client: &surge_ping::Client,
    address: IpAddr,
    timeout: Duration,
    count: u32,
) -> PingResult;

// dns.rs
pub async fn resolve_dns(
    resolver: &hickory_resolver::Resolver<hickory_resolver::name_server::TokioConnectionProvider>,
    hostname: &str,
    include_ipv6: bool,
) -> DnsResult;

// check.rs
pub async fn run_all_checks(
    config: &Config,
    parallel: bool,
) -> Vec<CheckResult>;
```

### Example Configuration

**~/.config/cxn/cxn.yml:**

```yaml
timeout_ms: 1000
retry_count: 3

hosts:
  - name: "Google DNS"
    address: "8.8.8.8"
    ping: true
    dns: false

  - name: "Cloudflare"
    address: "1.1.1.1"
    ping: true
    dns: false

  - name: "Google"
    address: "google.com"
    ping: true
    dns: true

  - name: "GitHub"
    address: "github.com"
    ping: false
    dns: true
```

### Expected Output

**`cxn` or `cxn check` (default command):**

```
Checking 4 hosts...

Google DNS (8.8.8.8)
  ✓ ping: 12.3ms

Cloudflare (1.1.1.1)
  ✓ ping: 8.7ms

Google (google.com)
  ✓ dns:  142.250.80.46, 2607:f8b0:4004:800::200e
  ✓ ping: 15.2ms

GitHub (github.com)
  ✓ dns:  140.82.112.3

Summary: 4/4 hosts OK
```

**`cxn check` with failures:**

```
Checking 4 hosts...

Google DNS (8.8.8.8)
  ✓ ping: 12.3ms

Cloudflare (1.1.1.1)
  ✗ ping: timeout after 1000ms

Google (google.com)
  ✓ dns:  142.250.80.46
  ✓ ping: 15.2ms

BadHost (nonexistent.invalid)
  ✗ dns:  no such host

Summary: 2/4 hosts OK, 2 failed
```

**`cxn ping 8.8.8.8`:**

```
PING 8.8.8.8
  64 bytes: seq=1 time=12.1ms
  64 bytes: seq=2 time=11.8ms
  64 bytes: seq=3 time=12.4ms
  64 bytes: seq=4 time=11.9ms

--- 8.8.8.8 ping statistics ---
4 packets transmitted, 4 received, 0% packet loss
rtt min/avg/max = 11.8/12.1/12.4 ms
```

**`cxn dns github.com`:**

```
github.com
  A:    140.82.112.3
  AAAA: (none)
```

### File Structure

After implementation, the project will have:

```
cxn/
├── Cargo.toml          # Updated with new dependencies
├── build.rs            # Existing (git describe version)
├── cxn.yml             # Sample config (updated schema)
├── src/
│   ├── main.rs         # Async entry point, command dispatch
│   ├── cli.rs          # Clap structs with Commands enum
│   ├── config.rs       # Config + HostConfig structs
│   ├── ping.rs         # NEW: surge-ping wrapper
│   ├── dns.rs          # NEW: hickory-resolver wrapper
│   ├── check.rs        # NEW: coordinated checks
│   └── output.rs       # NEW: colored output formatting
└── tests/
    └── integration.rs  # Integration tests
```

### Implementation Plan

**Phase 1: Foundation**
- Update `Cargo.toml` with new dependencies (tokio, surge-ping, hickory-resolver)
- Update `Config` struct with new schema
- Create sample `cxn.yml` with example hosts
- Convert `main.rs` to async with `#[tokio::main]`

**Phase 2: Ping Module**
- Create `src/ping.rs` with `ping_host` function
- Handle IPv4 and IPv6 addresses
- Implement timeout and retry logic
- Add colored output formatting

**Phase 3: DNS Module**
- Create `src/dns.rs` with `resolve_dns` function
- Support both A and AAAA record lookups
- Handle resolution failures gracefully

**Phase 4: CLI Enhancement**
- Add `Commands` enum with `Ping`, `Dns`, `Check` variants
- Implement subcommand dispatch in `main.rs`
- Add subcommand-specific argument parsing

**Phase 5: Check Module**
- Create `src/check.rs` for coordinated checks
- Implement parallel execution with `tokio::join!` or `JoinSet`
- Aggregate and display results

**Phase 6: Polish**
- Add summary statistics
- Improve error messages
- Update README with usage examples

## Alternatives Considered

### Alternative 1: Synchronous Execution

- **Description:** Use synchronous/blocking ping and DNS libraries instead of async
- **Pros:** Simpler code, no async runtime needed
- **Cons:** Sequential execution only, slower for multiple hosts
- **Why not chosen:** Parallel execution significantly improves UX when checking multiple hosts

### Alternative 2: External Command Wrapping

- **Description:** Shell out to system `ping` and `dig` commands
- **Pros:** No additional dependencies, uses system tools
- **Cons:** Platform-dependent output parsing, no control over behavior, harder error handling
- **Why not chosen:** Native Rust libraries provide consistent cross-platform behavior and better error handling

### Alternative 3: HTTP-based Connectivity

- **Description:** Use HTTP HEAD requests instead of ICMP ping
- **Pros:** Works without raw socket permissions, tests full stack
- **Cons:** Requires HTTP server on target, different semantics than ping
- **Why not chosen:** ICMP ping tests network-layer connectivity which is the goal; HTTP testing could be added later

## Technical Considerations

### Dependencies

**New crates to add:**

| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1.43+ | Async runtime |
| surge-ping | 0.8+ | ICMP ping |
| hickory-resolver | 0.24+ | DNS resolution |

**Existing crates retained:**
- clap 4.5 (CLI)
- serde + serde_yaml (config)
- colored (output)
- eyre (errors)
- log + env_logger (logging)

### Performance

- Parallel host checking via Tokio tasks
- Single `surge-ping::Client` instance reused across pings (socket reuse)
- Single `hickory_resolver::Resolver` instance with DNS caching
- Default timeout of 1000ms prevents hanging on unreachable hosts

### Security

**Raw Socket Permissions:**
`surge-ping` requires raw socket capabilities for ICMP. Options:
1. Run as root (not recommended for regular use)
2. Set capabilities: `sudo setcap cap_net_raw=eip ./cxn`
3. Document this requirement in README and error messages

**Configuration File:**
- Config file is read-only, no code execution
- Paths are validated before use
- No sensitive data in default config

### Testing Strategy

1. **Unit Tests:**
   - Config parsing with valid/invalid YAML
   - Result type formatting
   - CLI argument parsing

2. **Integration Tests:**
   - Ping localhost (127.0.0.1)
   - DNS resolution of known hostnames
   - Full `check` command with mock config

3. **Manual Testing:**
   - Test against real hosts (8.8.8.8, 1.1.1.1)
   - Test failure cases (unreachable hosts, invalid hostnames)
   - Test permission errors without raw socket capability

### Rollout Plan

1. Implement and test locally
2. Run `otto ci` to verify lint, check, test pass
3. Update README with installation and usage
4. Tag release v0.1.0
5. Consider publishing to crates.io

## Edge Cases and Error Handling

### Permission Errors

**Raw socket permission denied:**
```
Error: Permission denied (os error 1)

ICMP ping requires raw socket capabilities. Fix with one of:
  1. sudo setcap cap_net_raw=eip /path/to/cxn
  2. Run as root (not recommended)

See: https://man7.org/linux/man-pages/man7/capabilities.7.html
```

### Network Errors

| Error | Behavior |
|-------|----------|
| Timeout | Report as `✗ ping: timeout after {timeout_ms}ms` |
| Network unreachable | Report as `✗ ping: network unreachable` |
| Host unreachable | Report as `✗ ping: host unreachable` |
| No route to host | Report as `✗ ping: no route to host` |

### DNS Errors

| Error | Behavior |
|-------|----------|
| NXDOMAIN (no such host) | Report as `✗ dns: no such host` |
| SERVFAIL | Report as `✗ dns: server failure` |
| Timeout | Report as `✗ dns: timeout` |
| No IPv4/IPv6 records | Report available records; show `(none)` for missing type |

### Configuration Edge Cases

| Scenario | Behavior |
|----------|----------|
| Config file not found | Use defaults; show info message in verbose mode |
| Empty hosts list | Print "No hosts configured" and exit with code 0 |
| Invalid YAML syntax | Exit with error, show line number and context |
| `dns: true` on IP address | Skip DNS check silently (log in verbose mode) |
| Both `ping` and `dns` false | Skip host, warn in verbose mode |
| Hostname resolves but ping fails | Show DNS success, then ping failure separately |

### Interrupt Handling

- Ctrl+C (SIGINT): Cancel pending operations, print partial results, exit with code 130
- In parallel mode, cancel all in-flight requests immediately

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All checks passed (or no hosts configured) |
| 1 | One or more checks failed |
| 2 | Configuration error (invalid YAML, missing required fields) |
| 126 | Permission denied (raw socket) |
| 130 | Interrupted by user (Ctrl+C) |

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Raw socket permission issues | High | High | Clear error message with `setcap` instructions; document in README |
| DNS resolver initialization failure | Low | Medium | Fallback to system resolver config; graceful error handling |
| Tokio runtime conflicts | Low | Low | Use single runtime instance; avoid mixing runtimes |
| YAML parsing errors on malformed config | Medium | Low | Validate config on load; provide clear error messages with line numbers |
| IPv6-only host on IPv4-only system | Medium | Low | Attempt IPv4 first; fall back to IPv6; report specific error |
| Very large hosts list (100+) | Low | Medium | Use bounded concurrency (e.g., 20 parallel tasks max) |

## Open Questions

- [x] Should `check` be the default command when no subcommand given? **Yes, makes the common case easy**
- [ ] Should we support DNS-over-TLS/DNS-over-HTTPS? **Defer to future version**
- [ ] Should we add a `--json` output format? **Consider for v1.1**

## References

- [surge-ping crate](https://crates.io/crates/surge-ping)
- [hickory-resolver crate](https://crates.io/crates/hickory-resolver)
- [Tokio runtime](https://tokio.rs/)
- [Linux capabilities for raw sockets](https://man7.org/linux/man-pages/man7/capabilities.7.html)
