use crate::monitor::app::{App, HostState, ViewMode};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
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

            // Truncate host name if needed
            let max_name_len = 12;
            let display_name = if host.name.len() > max_name_len {
                format!("{}...", &host.name[..max_name_len - 3])
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
        Constraint::Length(14), // Host name
        Constraint::Length(6),  // Status
        Constraint::Length(8),  // Ping
        Constraint::Min(40),    // Data column (graph or latency)
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
            None => pad_to_width("×", 2), // Unicode-aware padding
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

/// Render the error log pane
fn render_error_log(frame: &mut Frame, app: &App, area: Rect) {
    let visible_errors: Vec<Line> = app
        .error_log
        .iter()
        .rev()
        .take(6)
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

    let error_block = Paragraph::new(error_text).block(
        Block::default()
            .title(format!(" Recent Errors ({}) ", app.error_log.len()))
            .borders(Borders::ALL)
            .border_style(if app.error_log.is_empty() {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Red)
            }),
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
    let help_text = " Tab view | ↑↓ scroll | c clear | q quit ";
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Right);

    frame.render_widget(stats, chunks[0]);
    frame.render_widget(help, chunks[1]);
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
        assert_eq!(pad_to_width("×", 2), " ×");
        assert_eq!(pad_to_width("ab", 2), "ab");
        assert_eq!(pad_to_width("a", 3), "  a");
    }
}
