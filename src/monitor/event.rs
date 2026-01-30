use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;

/// Events that can occur in the TUI
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Event {
    /// A key was pressed
    Key(KeyEvent),
    /// Time to run checks (tick interval)
    Tick,
    /// Terminal was resized
    Resize(u16, u16),
    /// Application should quit
    Quit,
}

/// Handles events from the terminal and timer
pub struct EventHandler {
    /// Channel sender for events
    _tx: mpsc::UnboundedSender<Event>,
    /// Channel receiver for events
    rx: mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    /// Create a new event handler
    ///
    /// * `render_rate` - How often to check for input events
    /// * `tick_rate` - How often to send tick events (for network checks)
    pub fn new(render_rate: Duration, tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn event loop task
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick_interval = interval(tick_rate);
            let mut render_interval = interval(render_rate);

            loop {
                tokio::select! {
                    // Handle terminal events
                    Some(Ok(event)) = reader.next() => {
                        match event {
                            CrosstermEvent::Key(key) => {
                                // Check for quit keys
                                if key.code == KeyCode::Char('q') ||
                                   (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
                                    let _ = tx_clone.send(Event::Quit);
                                } else {
                                    let _ = tx_clone.send(Event::Key(key));
                                }
                            }
                            CrosstermEvent::Resize(width, height) => {
                                let _ = tx_clone.send(Event::Resize(width, height));
                            }
                            _ => {}
                        }
                    }
                    // Handle tick interval
                    _ = tick_interval.tick() => {
                        let _ = tx_clone.send(Event::Tick);
                    }
                    // Handle render interval (used to keep event loop responsive)
                    _ = render_interval.tick() => {
                        // Just keeps the loop responsive for rendering
                    }
                }
            }
        });

        Self { _tx: tx, rx }
    }

    /// Get the event receiver channel
    pub fn subscribe(self) -> mpsc::UnboundedReceiver<Event> {
        self.rx
    }
}
