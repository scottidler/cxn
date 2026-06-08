use crate::monitor::app::{App, FocusPanel, HostState, ViewMode};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};
use unicode_width::UnicodeWidthStr;

/// Render the entire UI
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Min(5),    // Main table
        Constraint::Length(8), // Error log
        Constraint::Length(3), // Status bar
    ])
    .split(frame.area());

    render_host_table(frame, app, chunks[0]);
    render_error_log(frame, app, chunks[1]);
    render_status_bar(frame, app, chunks[2]);

    // Detail popup overlay (rendered last so it's on top)
    if let Some(host_idx) = app.detail_host {
        if let Some(host) = app.hosts.get(host_idx) {
            render_host_detail_popup(frame, app, host);
        }
    }
}

/// Render the host table with tab-switchable view: Graph (default) or Latency
fn render_host_table(frame: &mut Frame, app: &App, area: Rect) {
    // Build header based on view mode
    let data_header = match app.view_mode {
        ViewMode::Graph => "Graph",
        ViewMode::Latency => "Recent (ms)",
    };

    let header = Row::new(vec![
        Cell::from("Host").style(Style::default().fg(Color::DarkGray)),
        Cell::from("Status").style(Style::default().fg(Color::DarkGray)),
        Cell::from("Ping").style(Style::default().fg(Color::DarkGray)),
        Cell::from(data_header).style(Style::default().fg(Color::DarkGray)),
    ])
    .height(1);

    // Dynamic host name column width based on longest hostname
    let min_host_width: u16 = 8;
    let max_host_width: u16 = 30;
    let longest_name = app.hosts.iter().map(|h| h.name.len()).max().unwrap_or(4);
    let host_col_width = ((longest_name as u16) + 2).clamp(min_host_width, max_host_width);

    let rows: Vec<Row> = app
        .hosts
        .iter()
        .enumerate()
        .map(|(idx, host)| {
            let is_selected = idx == app.selected_host;
            let is_error = host.last_sample.as_ref().is_some_and(|s| !s.is_success());

            // Determine row style
            let row_style = if is_error {
                Style::default().fg(Color::Red)
            } else if is_selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            // Status cell with color
            let status_style = match host.last_sample.as_ref().map(|s| s.is_success()) {
                None => Style::default().fg(Color::DarkGray),
                Some(true) => Style::default().fg(Color::Green),
                Some(false) => Style::default().fg(Color::Red),
            };

            // Ping cell with color
            let ping_text = host.ping_display();
            let ping_style = if ping_text == "FAIL" {
                Style::default().fg(Color::Red)
            } else if ping_text == "-" {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Green)
            };

            // Truncate host name if needed (dynamic based on column width)
            let max_name_len = host_col_width as usize;
            let display_name = if host.name.len() > max_name_len && max_name_len > 1 {
                let truncated: String = host.name.chars().take(max_name_len - 1).collect();
                format!("{truncated}\u{2026}")
            } else {
                host.name.clone()
            };

            // Get data column based on view mode
            let data_cell = match app.view_mode {
                ViewMode::Graph => render_braille_graph(host, 40),
                ViewMode::Latency => render_latency_values(host, 15),
            };

            Row::new(vec![
                Cell::from(display_name),
                Cell::from(host.status_symbol()).style(status_style),
                Cell::from(ping_text).style(ping_style),
                Cell::from(data_cell),
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Length(host_col_width), // Host name (dynamic)
        Constraint::Length(6),              // Status
        Constraint::Length(8),              // Ping
        Constraint::Min(40),                // Data column (graph or latency)
    ];

    // Title shows active tab with indicator
    let title = format!(" CXN Network Monitor  [{}]  (Tab to switch) ", app.view_mode.title());

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(table, area);
}

/// Render recent latency values as scrolling numbers (newest on left, scrolls right)
fn render_latency_values(host: &HostState, count: usize) -> String {
    let data = host.latency_values();
    if data.is_empty() {
        return "-".to_string();
    }

    // Take the most recent values, reversed (newest first/left)
    let values: Vec<_> = data.iter().rev().take(count).collect();

    values
        .iter()
        .map(|v| match v {
            Some(ms) => {
                if *ms < 100 {
                    format!("{:2}", ms)
                } else {
                    format!("{:3}", ms.min(&999))
                }
            }
            None => pad_to_width("\u{00d7}", 2), // Unicode-aware padding
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pad a string to a target display width using Unicode-aware width calculation
fn pad_to_width(s: &str, width: usize) -> String {
    let current_width = s.width();
    if current_width >= width {
        s.to_string()
    } else {
        format!("{}{}", " ".repeat(width - current_width), s)
    }
}

/// Render a braille graph like btop
/// Uses Unicode braille characters (U+2800 to U+28FF)
/// Each character is a 2x4 dot matrix, allowing 4 vertical levels per column
/// Data flows: newest on left, scrolls right
fn render_braille_graph(host: &HostState, width: usize) -> String {
    let data = host.sparkline_data();
    if data.is_empty() {
        return String::new();
    }

    // We need width * 2 data points (2 columns per braille character)
    let needed = width * 2;
    let available: Vec<u64> = if data.len() >= needed {
        // Take most recent, then reverse so newest is first
        data.iter().rev().take(needed).copied().collect()
    } else {
        // Data is newest-first, pad with zeros on the right
        let mut result: Vec<u64> = data.iter().rev().copied().collect();
        result.resize(needed, 0);
        result
    };

    // Find min/max for scaling
    let max = *available.iter().filter(|&&v| v > 0).max().unwrap_or(&100);
    let min = 0u64; // Always start from 0 for graphs

    // Scale to 0-7 (8 levels for 4 dots * 2 visual states)
    // Actually braille has 4 dots vertically, so we scale to 0-4
    let scaled: Vec<u8> = available
        .iter()
        .map(|&v| {
            if v == 0 {
                0
            } else if max == min {
                2 // Middle height if all same
            } else {
                let normalized = ((v - min) as f64 / (max - min) as f64 * 4.0).round() as u8;
                normalized.clamp(1, 4) // At least 1 dot if there's data
            }
        })
        .collect();

    // Convert pairs of values to braille characters
    // Braille dot positions:
    //   1  4    (bits 0, 3)
    //   2  5    (bits 1, 4)
    //   3  6    (bits 2, 5)
    //   7  8    (bits 6, 7)
    //
    // For a bar graph, we fill from bottom up:
    // Level 4: all dots (7,8 + 3,6 + 2,5 + 1,4)
    // Level 3: 7,8 + 3,6 + 2,5
    // Level 2: 7,8 + 3,6
    // Level 1: 7,8
    // Level 0: empty

    let mut result = String::with_capacity(width);

    for chunk in scaled.chunks(2) {
        let left = chunk.first().copied().unwrap_or(0);
        let right = chunk.get(1).copied().unwrap_or(0);

        let braille = braille_char(left, right);
        result.push(braille);
    }

    result
}

/// Convert two height values (0-4) to a braille character
/// Fills dots from bottom up for each column
fn braille_char(left: u8, right: u8) -> char {
    // Braille dot bits:
    //   Dot 1 (bit 0) - top left
    //   Dot 2 (bit 1) - second from top, left
    //   Dot 3 (bit 2) - third from top, left
    //   Dot 7 (bit 6) - bottom left
    //   Dot 4 (bit 3) - top right
    //   Dot 5 (bit 4) - second from top, right
    //   Dot 6 (bit 5) - third from top, right
    //   Dot 8 (bit 7) - bottom right

    // For bar graph from bottom up:
    // Height 4: dots 7,3,2,1 (bits 6,2,1,0) = 0b01000111 = 0x47
    // Height 3: dots 7,3,2   (bits 6,2,1)   = 0b01000110 = 0x46
    // Height 2: dots 7,3     (bits 6,2)     = 0b01000100 = 0x44
    // Height 1: dot  7       (bit 6)        = 0b01000000 = 0x40
    // Height 0: empty                       = 0b00000000 = 0x00

    let left_bits = match left {
        4 => 0b01000111u8, // dots 7,3,2,1
        3 => 0b01000110u8, // dots 7,3,2
        2 => 0b01000100u8, // dots 7,3
        1 => 0b01000000u8, // dot 7
        _ => 0u8,
    };

    let right_bits = match right {
        4 => 0b10111000u8, // dots 8,6,5,4
        3 => 0b10110000u8, // dots 8,6,5
        2 => 0b10100000u8, // dots 8,6
        1 => 0b10000000u8, // dot 8
        _ => 0u8,
    };

    let code_point = 0x2800u32 + (left_bits | right_bits) as u32;
    char::from_u32(code_point).unwrap_or(' ')
}

/// Render the error log pane with scrolling support
fn render_error_log(frame: &mut Frame, app: &App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize; // border top + bottom
    let total = app.error_log.len();

    let visible_errors: Vec<Line> = app
        .error_log
        .iter()
        .rev()
        .skip(app.error_scroll_offset)
        .take(inner_height)
        .map(|e| {
            Line::from(vec![
                Span::styled(
                    format!("{} ", e.timestamp.format("%H:%M:%S")),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{:12} ", e.host), Style::default().fg(Color::Yellow)),
                Span::styled(&e.message, Style::default().fg(Color::Red)),
            ])
        })
        .collect();

    let error_text = if visible_errors.is_empty() {
        vec![Line::from(Span::styled(
            "No errors",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        visible_errors
    };

    let scroll_indicator = if total > inner_height {
        let pos = app.error_scroll_offset + 1;
        let end = (app.error_scroll_offset + inner_height).min(total);
        format!(" Recent Errors ({}) [{}-{}/{}] ", total, pos, end, total)
    } else {
        format!(" Recent Errors ({}) ", total)
    };

    let is_focused = app.focus == FocusPanel::ErrorLog;
    let border_color = if is_focused {
        Color::Yellow
    } else if app.error_log.is_empty() {
        Color::DarkGray
    } else {
        Color::Red
    };

    let error_block = Paragraph::new(error_text).block(
        Block::default()
            .title(scroll_indicator)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );

    frame.render_widget(error_block, area);
}

/// Render the status bar
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)]).split(area);

    // Left side: statistics
    let stats_text = format!(
        " Checks: {} | Errors: {} | Uptime: {} | Interval: {}s",
        app.stats.total_checks,
        app.stats.total_errors,
        app.stats.uptime_display(),
        app.interval.as_secs()
    );

    let stats = Paragraph::new(stats_text).style(Style::default().fg(Color::Cyan));

    // Right side: help
    let help_text = " Tab view | \u{2191}\u{2193} nav | e errors | Enter detail | c clear | q quit ";
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Right);

    frame.render_widget(stats, chunks[0]);
    frame.render_widget(help, chunks[1]);
}

/// Centered popup area helper
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

/// Render the host detail popup overlay
fn render_host_detail_popup(frame: &mut Frame, app: &App, host: &HostState) {
    let area = centered_rect(70, 70, frame.area());

    // Clear the area behind the popup
    frame.render_widget(Clear, area);

    let mut lines: Vec<Line> = Vec::new();

    // Header: host name and address
    lines.push(Line::from(vec![
        Span::styled("Host: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(&host.name),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "Address: ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(&host.address),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "Status: ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            host.status_symbol(),
            if host.last_sample.as_ref().is_some_and(|s| s.is_success()) {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            },
        ),
    ]));

    // Resolved IPs from last DNS result
    if let Some(ref sample) = host.last_sample {
        if let Some(ref dns) = sample.dns_result {
            let ips: Vec<String> = dns.addresses.iter().map(|ip| ip.to_string()).collect();
            lines.push(Line::from(vec![
                Span::styled("IPs: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(if ips.is_empty() { "-".to_string() } else { ips.join(", ") }),
            ]));
        }
    }

    lines.push(Line::from(vec![
        Span::styled(
            "Checks: ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{}", host.success_count), Style::default().fg(Color::Green)),
        Span::raw(" ok / "),
        Span::styled(format!("{}", host.error_count), Style::default().fg(Color::Red)),
        Span::raw(" err"),
    ]));

    lines.push(Line::from(""));

    // Latency history
    lines.push(Line::from(Span::styled(
        "\u{2500}\u{2500} Latency History \u{2500}\u{2500}",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));

    let latencies = host.latency_values();
    if latencies.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No data yet",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Show latencies in rows of ~20 values
        let chunk_size = 20;
        for chunk in latencies.chunks(chunk_size) {
            let vals: Vec<String> = chunk
                .iter()
                .map(|v| match v {
                    Some(ms) => format!("{:>4}", ms),
                    None => "   \u{00d7}".to_string(),
                })
                .collect();
            lines.push(Line::from(Span::raw(format!("  {}", vals.join(" ")))));
        }
    }

    lines.push(Line::from(""));

    // Recent errors for this host
    lines.push(Line::from(Span::styled(
        "\u{2500}\u{2500} Recent Errors \u{2500}\u{2500}",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));

    let host_errors: Vec<&crate::monitor::app::ErrorEntry> = app
        .error_log
        .iter()
        .rev()
        .filter(|e| e.host == host.name)
        .take(10)
        .collect();

    if host_errors.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No errors",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for e in host_errors {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", e.timestamp.format("%H:%M:%S")),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(&e.message, Style::default().fg(Color::Red)),
            ]));
        }
    }

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .title(format!(" {} Detail ", host.name))
                .title_bottom(" Esc/q to close ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(popup, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_braille_char_empty() {
        let c = braille_char(0, 0);
        assert_eq!(c, '\u{2800}'); // Empty braille
    }

    #[test]
    fn test_braille_char_full() {
        let c = braille_char(4, 4);
        // Should have all bottom-up dots filled
        assert!(('\u{2800}'..='\u{28FF}').contains(&c));
    }

    #[test]
    fn test_braille_char_half() {
        let c = braille_char(2, 2);
        assert!(('\u{2800}'..='\u{28FF}').contains(&c));
    }

    #[test]
    fn test_pad_to_width() {
        assert_eq!(pad_to_width("\u{00d7}", 2), " \u{00d7}");
        assert_eq!(pad_to_width("ab", 2), "ab");
        assert_eq!(pad_to_width("a", 3), "  a");
    }
}
