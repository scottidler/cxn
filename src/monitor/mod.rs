mod app;
mod event;
mod network;
mod tui;
mod ui;

pub use app::App;
pub use event::{Event, EventHandler};
pub use network::spawn_check_tasks;
pub use tui::Tui;

use crate::cli::MonitorArgs;
use crate::config::Config;
use eyre::Result;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Run the monitor TUI
pub async fn run_monitor(config: Config, args: MonitorArgs) -> Result<()> {
    // Resolve interval and history from args or config
    let interval = args.interval.unwrap_or(config.interval);
    let history = args.history.unwrap_or(config.history);

    // Check if running in a TTY
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return Err(eyre::eyre!("Monitor mode requires an interactive terminal"));
    }

    // Initialize application state
    let mut app = App::new(&config, history);

    // Initialize terminal
    let mut tui = Tui::new()?;

    // Create channels for communication
    let (result_tx, mut result_rx) = mpsc::unbounded_channel();

    // Create cancellation token for graceful shutdown
    let cancel_token = CancellationToken::new();

    // Spawn event handler
    let event_handler = EventHandler::new(Duration::from_millis(100), Duration::from_secs(interval));
    let mut event_rx = event_handler.subscribe();

    // Spawn network check tasks
    let _check_handles = spawn_check_tasks(
        config.hosts(),
        Duration::from_secs(interval),
        Duration::from_millis(config.timeout),
        result_tx,
        cancel_token.clone(),
    );

    // Main event loop
    loop {
        // Render current state
        tui.draw(|frame| ui::render(frame, &app))?;

        // Handle events
        tokio::select! {
            Some(event) = event_rx.recv() => {
                match event {
                    Event::Key(key) => {
                        app.handle_key(key);
                    }
                    Event::Tick => {
                        // Tick events are handled by check tasks
                    }
                    Event::Resize(_, _) => {
                        // Resize is handled automatically by ratatui
                    }
                    Event::Quit => {
                        break;
                    }
                }
            }
            Some((host_name, sample)) = result_rx.recv() => {
                app.record_sample(&host_name, sample);
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Cancel all check tasks
    cancel_token.cancel();

    // Restore terminal
    tui.exit()?;

    Ok(())
}
