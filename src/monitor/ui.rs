use crate::monitor::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

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

/// Render the host table with sparklines
fn render_host_table(frame: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Host").style(Style::default().fg(Color::DarkGray)),
        Cell::from("Status").style(Style::default().fg(Color::DarkGray)),
        Cell::from("Ping").style(Style::default().fg(Color::DarkGray)),
        Cell::from("DNS").style(Style::default().fg(Color::DarkGray)),
        Cell::from("History").style(Style::default().fg(Color::DarkGray)),
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

            // DNS cell with color
            let dns_text = host.dns_display();
            let dns_style = if dns_text == "FAIL" {
                Style::default().fg(Color::Red)
            } else if dns_text == "-" {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Green)
            };

            // Truncate host name if needed
            let max_name_len = 14;
            let display_name = if host.name.len() > max_name_len {
                format!("{}...", &host.name[..max_name_len - 3])
            } else {
                host.name.clone()
            };

            Row::new(vec![
                Cell::from(display_name),
                Cell::from(host.status_symbol()).style(status_style),
                Cell::from(ping_text).style(ping_style),
                Cell::from(dns_text).style(dns_style),
                Cell::from(render_inline_sparkline(host)),
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Length(16), // Host name
        Constraint::Length(8),  // Status
        Constraint::Length(10), // Ping
        Constraint::Length(10), // DNS
        Constraint::Min(20),    // Sparkline
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(" CXN Network Monitor ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(table, area);
}

/// Render an inline sparkline as a text string
fn render_inline_sparkline(host: &crate::monitor::app::HostState) -> String {
    const BARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    let data = host.sparkline_data();
    if data.is_empty() {
        return String::new();
    }

    let max = *data.iter().max().unwrap_or(&1);
    let min = *data.iter().min().unwrap_or(&0);
    let range = if max == min { 1 } else { max - min };

    data.iter()
        .map(|&val| {
            let normalized = ((val - min) as f64 / range as f64 * 7.0) as usize;
            let idx = normalized.min(7);
            BARS[idx]
        })
        .collect()
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
    let help_text = " 'q' quit | ↑/↓ scroll | 'c' clear ";
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Right);

    frame.render_widget(stats, chunks[0]);
    frame.render_widget(help, chunks[1]);
}
