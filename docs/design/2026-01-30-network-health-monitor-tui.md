# Design Document: CXN Network Health Monitor TUI

**Author:** Claude (with Scott)
**Date:** 2026-01-30
**Status:** Implementation Complete - Ready for Testing
**Review Passes Completed:** 5/5

## Summary

Transform `cxn` from a one-shot connectivity checker into a continuous network health monitor TUI. The application runs indefinitely, performing periodic DNS lookups and ICMP pings against configured hosts, displaying results with rolling history sparklines, and prominently alerting on failures.

## Problem Statement

### Background

Scott's home network experiences intermittent failures. Currently monitored by running `ping 10.10.10.1` and `host google.com` in separate tmux panes. The existing `cxn` tool provides one-shot connectivity snapshots with an optional `--watch` mode, but lacks:
- Historical data visualization (can't see trends)
- Prominent error alerting (easy to miss failures)
- Efficient continuous monitoring

### Problem

Need a persistent, visual network health dashboard that:
1. Continuously monitors multiple hosts in parallel
2. Shows historical trends via sparklines
3. Immediately highlights failures/errors
4. Runs indefinitely with minimal resource usage

### Goals

- Replace tmux-based monitoring with single unified TUI
- Show rolling history of ping latencies (last N samples per host)
- Display sparklines for visual trend identification
- Prominently highlight DNS failures, ping timeouts, and errors
- Support existing `cxn.yml` configuration format
- Maintain low CPU/memory footprint for always-on operation

### Non-Goals

- Sound/desktop notifications
- Log file persistence of historical data
- Remote monitoring/multi-machine aggregation
- Custom alerting rules/thresholds

## Implementation Status

### Completed Tasks

| # | Task | Status | Files Modified |
|---|------|--------|----------------|
| 1 | Add TUI dependencies to Cargo.toml | ✅ Complete | `Cargo.toml` |
| 2 | Add Monitor subcommand to CLI | ✅ Complete | `src/cli.rs` |
| 3 | Add history field to Config | ✅ Complete | `src/config.rs` |
| 4 | Add timed DNS resolution | ✅ Complete | `src/dns.rs` |
| 5 | Create monitor module structure | ✅ Complete | `src/monitor/mod.rs` |
| 6 | Implement Tui terminal wrapper | ✅ Complete | `src/monitor/tui.rs` |
| 7 | Implement App state and RingBuffer | ✅ Complete | `src/monitor/app.rs` |
| 8 | Implement Event handling | ✅ Complete | `src/monitor/event.rs` |
| 9 | Implement network check spawner | ✅ Complete | `src/monitor/network.rs` |
| 10 | Implement TUI rendering | ✅ Complete | `src/monitor/ui.rs` |
| 11 | Wire up monitor command in main.rs | ✅ Complete | `src/main.rs` |
| 12 | Build and test verification | ✅ Complete | - |

### Remaining Tasks

| # | Task | Status | Priority |
|---|------|--------|----------|
| 13 | Manual testing with real network | ⏳ Pending | High |
| 14 | Test error scenarios (network down) | ⏳ Pending | High |
| 15 | Test terminal resize handling | ⏳ Pending | Medium |
| 16 | Test Ctrl+C graceful shutdown | ⏳ Pending | High |
| 17 | Add integration tests | ⏳ Pending | Low |
| 18 | Performance profiling | ⏳ Pending | Low |

## Proposed Solution

### Overview

New `monitor` subcommand launches a ratatui-based TUI. Architecture follows TEA (The Elm Architecture) pattern with tokio handling async network operations and mpsc channels communicating results to the TUI rendering thread.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Main Thread                            │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              Application State (Model)                │  │
│  │  - hosts: Vec<HostState>                              │  │
│  │  - history: RingBuffer<Sample> per host               │  │
│  │  - error_log: VecDeque<ErrorEntry>                    │  │
│  │  - stats: GlobalStats                                 │  │
│  └───────────────────────────────────────────────────────┘  │
│                          ▲                                  │
│                          │ update()                         │
│                          │                                  │
│  ┌───────────────────────┴───────────────────────────────┐  │
│  │              Event Loop (tokio::select!)              │  │
│  │  - event_rx.recv() → keyboard/tick events             │  │
│  │  - result_rx.recv() → network check results           │  │
│  └───────────────────────────────────────────────────────┘  │
│                          │                                  │
│                          ▼ view()                           │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              terminal.draw(|frame| ...)               │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                           ▲
                           │ mpsc channels
┌──────────────────────────┴──────────────────────────────────┐
│                    Tokio Runtime                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │ Event Task  │  │ Check Task  │  │ Check Task  │  ...    │
│  │ (keyboard)  │  │ (host 1)    │  │ (host 2)    │         │
│  └─────────────┘  └─────────────┘  └─────────────┘         │
└─────────────────────────────────────────────────────────────┘
```

### Data Model

```rust
// Ring buffer for sample history
pub struct RingBuffer<T> {
    data: VecDeque<T>,
    capacity: usize,
}

// DNS check result
pub struct DnsCheckResult {
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub addresses: Vec<IpAddr>,
    pub error: Option<String>,
}

// Ping check result
pub struct PingCheckResult {
    pub success: bool,
    pub rtt_ms: Option<u64>,
    pub error: Option<String>,
}

// Single check result
pub struct Sample {
    pub timestamp: Instant,
    pub dns_result: Option<DnsCheckResult>,
    pub ping_result: Option<PingCheckResult>,
}

// Per-host state
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

// Error log entry
pub struct ErrorEntry {
    pub timestamp: DateTime<Local>,
    pub host: String,
    pub error_type: ErrorType,
    pub message: String,
}

// Main application state
pub struct App {
    pub hosts: Vec<HostState>,
    pub error_log: VecDeque<ErrorEntry>,
    pub stats: GlobalStats,
    pub selected_host: usize,
    pub should_quit: bool,
    pub interval: Duration,
}
```

### TUI Layout

```
┌─ CXN Network Monitor ─────────────────────────────────────────┐
│ Host           │ Status │ Ping   │ DNS    │ History           │
├────────────────┼────────┼────────┼────────┼───────────────────┤
│ Google DNS     │   ✓    │   8ms  │   -    │ ▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁ │
│ google.com     │   ✓    │  14ms  │  12ms  │ ▁▂▃▂▁▂▁▃▂▁▂▁▂▃▂▁ │
│ cloudflare     │   ✗    │  ---   │ FAIL   │ ▁▁▁▁▁▁▁▁████████ │ ← RED
├────────────────────────────────────────────────────────────────┤
│ Recent Errors (3)                                             │
│ 08:34:12  cloudflare    DNS timeout: no response             │
│ 08:34:07  cloudflare    DNS timeout: no response             │
├────────────────────────────────────────────────────────────────┤
│ Checks: 1,234 | Errors: 12 | Uptime: 2h 34m | Interval: 5s   │
│                              'q' quit | ↑/↓ scroll | 'c' clear │
└────────────────────────────────────────────────────────────────┘
```

### API Design

**CLI Interface:**
```bash
# Launch monitor with defaults from config
cxn monitor

# Override interval (check every 2 seconds)
cxn monitor -i 2

# Override history length (120 samples for sparklines)
cxn monitor -H 120

# Use specific config file
cxn monitor -c /path/to/config.yml
```

**Config Extension:**
```yaml
# cxn.yml
timeout: 1000
retries: 3
interval: 5      # seconds between checks
history: 60      # NEW: samples per host for sparklines
hosts:
  Router:
    address: "10.10.10.1"
    ping: true
```

### Files Created/Modified

| File | Change Type | Purpose |
|------|-------------|---------|
| `Cargo.toml` | Modified | Added ratatui, crossterm, futures, tokio-util |
| `src/cli.rs` | Modified | Added Monitor subcommand and MonitorArgs |
| `src/config.rs` | Modified | Added history field with default 60 |
| `src/dns.rs` | Modified | Added resolve_dns_timed() function |
| `src/main.rs` | Modified | Added monitor module and cmd dispatch |
| `src/monitor/mod.rs` | Created | Module exports and run_monitor() main loop |
| `src/monitor/tui.rs` | Created | Terminal wrapper with panic handling |
| `src/monitor/app.rs` | Created | App state, RingBuffer, HostState, Sample |
| `src/monitor/event.rs` | Created | Event enum and EventHandler (spawns keyboard listener) |
| `src/monitor/network.rs` | Created | Check task spawner (one task per host) |
| `src/monitor/ui.rs` | Created | TUI rendering (table, sparklines, error log, status bar) |

### Module Responsibilities

```
src/monitor/
├── mod.rs        # Orchestrates the main event loop:
│                 # 1. Creates App state from config
│                 # 2. Initializes terminal (Tui)
│                 # 3. Spawns EventHandler for keyboard
│                 # 4. Spawns check tasks for each host
│                 # 5. Runs tokio::select! loop for events/results
│
├── app.rs        # Pure state management (no I/O):
│                 # - RingBuffer<T> for fixed-size history
│                 # - Sample, DnsCheckResult, PingCheckResult
│                 # - HostState with sparkline_data() method
│                 # - App with handle_key() and record_sample()
│
├── event.rs      # Keyboard/terminal event handling:
│                 # - Spawns async task with crossterm::EventStream
│                 # - Sends Event variants through mpsc channel
│                 # - Handles quit keys (q, Ctrl+C) specially
│
├── network.rs    # Network check execution:
│                 # - spawn_check_tasks() creates one task per host
│                 # - Each task loops: check → send result → sleep
│                 # - Uses CancellationToken for graceful shutdown
│
├── tui.rs        # Terminal lifecycle management:
│                 # - Raw mode, alternate screen setup
│                 # - Panic hook to restore terminal
│                 # - Drop impl as safety net
│
└── ui.rs         # Rendering logic (pure functions):
                  # - render() splits into 3 panes
                  # - render_host_table() with inline sparklines
                  # - render_error_log() shows recent errors
                  # - render_status_bar() shows stats + help
```

## Alternatives Considered

### Alternative 1: Extend Watch Mode
- **Description:** Enhance existing `--watch` with sparklines using raw terminal escape codes
- **Pros:** Minimal changes, no new dependencies
- **Cons:** No proper TUI, no keyboard handling, limited layout options
- **Why not chosen:** Would be hacky; ratatui provides proper widget system

### Alternative 2: Web UI
- **Description:** Serve monitoring dashboard via web browser
- **Pros:** Rich visualization, accessible from any device
- **Cons:** Heavier resources, requires browser, loses terminal workflow
- **Why not chosen:** User wants terminal-native solution

### Alternative 3: Daemon Architecture
- **Description:** Fork a daemon process for network checks
- **Pros:** Complete isolation, survives TUI crashes
- **Cons:** Complex IPC, harder to debug
- **Why not chosen:** Tokio provides sufficient async isolation

## Technical Considerations

### Dependencies

**Added:**
- `ratatui 0.29` - TUI framework with crossterm backend
- `crossterm 0.28` - Terminal backend with event-stream feature
- `futures 0.3` - For StreamExt on EventStream
- `tokio-util 0.7` - For CancellationToken

**Existing (unchanged):**
- `tokio 1.43` - Async runtime
- `hickory-resolver` - DNS lookups
- `surge-ping` - ICMP ping

### Performance

- **Memory:** RingBuffer of 60 samples × N hosts × ~100 bytes ≈ negligible
- **CPU:** Checks run on intervals (default 5s), rendering on demand
- **Network:** Same as current watch mode (one check cycle per interval)

### Testing Strategy

**Unit tests (implemented):**
- RingBuffer operations
- GlobalStats uptime display

**Manual tests (pending):**
- Launch with sample config
- Verify sparklines update over time
- Test error display on network failure
- Test keyboard navigation
- Test graceful shutdown

### Rollout Plan

1. Implementation complete as new `monitor` subcommand (non-breaking)
2. Existing `check` and `check --watch` remain unchanged
3. Users can adopt `cxn monitor` when ready

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Terminal not restored on panic | Medium | High | panic::set_hook() + Drop impl |
| Permission denied for ping | Medium | Medium | Clear error message, show DNS only |
| High CPU from rendering | Low | Low | Render only on events/state change |
| Channel backup on load | Low | Low | Unbounded channels, drain on render |
| No hosts in config | Low | Low | Empty table shown, no crash |
| SSH session disconnect | Medium | Medium | Terminal restored by Drop impl |
| Very long host names | Low | Low | Truncated to 14 chars with "..." |

## Edge Cases Handled

| Scenario | Behavior |
|----------|----------|
| Empty config (no hosts) | Shows empty table, runs without error |
| Host with no ping or dns | Skipped by network spawner |
| DNS fails but ping succeeds | Row shows partial failure |
| All checks fail on startup | Shows error state immediately |
| Network goes down mid-run | Errors accumulate, sparklines show failure streak |
| Terminal resized | Layout recalculates automatically (ratatui handles) |
| Non-TTY stdout | Error: "Monitor mode requires an interactive terminal" |
| Ctrl+C during check | CancellationToken stops in-flight checks |

## Known Limitations

1. **No host selection visual** - Arrow keys update `selected_host` but no highlight shown
2. **No scrolling for many hosts** - Table doesn't scroll if hosts exceed terminal height
3. **Sparkline shows ping only** - DNS latency not visualized in sparkline
4. **No persistent state** - History lost on exit
5. **Error log not scrollable** - Shows only last 6 errors in view

## Verification Commands

```bash
# Build
cargo build

# Run tests
cargo test

# View help
cxn monitor --help

# Run monitor (requires config with hosts)
# Note: ICMP ping requires CAP_NET_RAW capability or root
cxn monitor

# Run with overrides
cxn monitor -i 2 -H 120

# If ping permission denied, either:
# 1. Run with sudo: sudo cxn monitor
# 2. Set capability: sudo setcap cap_net_raw+ep target/debug/cxn
```

## Open Questions

- [ ] Should sparklines show DNS latency instead of/in addition to ping?
- [ ] Should there be a way to force an immediate recheck (r key)?
- [ ] Should the error log be scrollable?
- [ ] Should there be a detail view for a selected host?
- [ ] Should row selection be visually highlighted?
- [ ] Should the table scroll when there are many hosts?

## Future Enhancements (Out of Scope)

These were identified during implementation but are not part of the initial release:

1. **Sound/desktop notifications** - Alert on state transitions
2. **Persistent history** - Save/load sparkline data across restarts
3. **Detail view** - Press Enter on selected host to see full history
4. **Configurable thresholds** - Red/yellow/green based on latency
5. **Export functionality** - Dump current state to JSON/CSV

## References

- [Ratatui Documentation](https://ratatui.rs/)
- [Ratatui Async Event Stream Tutorial](https://ratatui.rs/tutorials/counter-async-app/async-event-stream/)
- [The Elm Architecture Pattern](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/)
