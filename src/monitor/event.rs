use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;

/// Events that can occur in the TUI
#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyEvent),
    /// Periodic tick for re-rendering and data refresh
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
    /// * `tick_rate` - How often to send tick events (drives periodic re-rendering)
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn event loop task
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick_interval = interval(tick_rate);

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
                    // Tick drives periodic re-renders
                    _ = tick_interval.tick() => {
                        let _ = tx_clone.send(Event::Tick);
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
