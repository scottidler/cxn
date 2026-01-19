# cxn

A fast CLI tool for checking ping and DNS connectivity to multiple hosts.

## Features

- Parallel host checking (20 concurrent by default)
- ICMP ping with RTT measurements
- DNS resolution with IPv4/IPv6 support
- YAML configuration for host lists
- Colored terminal output
- Exit codes for scripting

## Installation

```bash
cargo install --path .
```

**Note:** ICMP ping requires raw socket permissions. On Linux, either run as root or grant capabilities:

```bash
sudo setcap cap_net_raw+ep ~/.cargo/bin/cxn
```

## Usage

### Check configured hosts (default)

```bash
# Check all hosts from config file
cxn

# Run checks sequentially instead of in parallel
cxn check --sequential

# Use a specific config file
cxn -c /path/to/config.yml
```

### Ping a host

```bash
# Ping with defaults (4 packets, 1000ms timeout)
cxn ping google.com

# Ping with custom count and timeout
cxn ping 8.8.8.8 -n 10 --timeout 2000
```

### DNS lookup

```bash
# Resolve hostname (IPv4 only)
cxn dns google.com

# Include IPv6 addresses
cxn dns google.com -6
```

## Configuration

Configuration is loaded from (in order of precedence):
1. Path specified with `-c`/`--config`
2. `./cxn.yml` (current directory)
3. `~/.config/cxn/cxn.yml`

### Example configuration

```yaml
timeout_ms: 1000
retry_count: 1

hosts:
  - name: Google DNS
    address: 8.8.8.8
    ping: true

  - name: Cloudflare DNS
    address: 1.1.1.1
    ping: true

  - name: Google
    address: google.com
    ping: true
    dns: true

  - name: GitHub
    address: github.com
    dns: true
```

### Host options

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | Display name for the host |
| `address` | string | required | IP address or hostname |
| `ping` | bool | false | Enable ICMP ping check |
| `dns` | bool | false | Enable DNS resolution check |

## Output

```
$ cxn
Checking 4 hosts...

Google DNS (8.8.8.8)
  ✓ ping: 12.3ms

Cloudflare DNS (1.1.1.1)
  ✓ ping: 8.7ms

Google (google.com)
  ✓ dns:  142.250.80.46
  ✓ ping: 15.2ms

GitHub (github.com)
  ✓ dns:  140.82.114.3

Summary: 4/4 hosts OK in 1.2s
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All checks passed |
| 1 | One or more checks failed |

## Logs

Logs are written to `~/.local/share/cxn/logs/cxn.log`

## License

MIT
