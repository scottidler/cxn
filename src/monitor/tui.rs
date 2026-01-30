use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use eyre::{Context, Result};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout};
use std::panic;

/// Terminal User Interface wrapper
///
/// Handles terminal setup, teardown, and provides a safe interface for rendering.
/// Ensures terminal is restored even on panic.
pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Tui {
    /// Create a new TUI instance
    ///
    /// Enters raw mode, switches to alternate screen, and sets up panic handling
    /// to restore terminal state.
    pub fn new() -> Result<Self> {
        // Set up panic hook to restore terminal on panic
        let original_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic_info| {
            // Restore terminal state
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            // Call original panic hook
            original_hook(panic_info);
        }));

        // Enter raw mode and alternate screen
        enable_raw_mode().context("Failed to enable raw mode")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;

        // Create terminal
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).context("Failed to create terminal")?;

        Ok(Self { terminal })
    }

    /// Draw to the terminal
    ///
    /// Wraps the terminal's draw method with proper error handling.
    pub fn draw<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.terminal.draw(f).context("Failed to draw to terminal")?;
        Ok(())
    }

    /// Exit the TUI and restore terminal state
    pub fn exit(&mut self) -> Result<()> {
        disable_raw_mode().context("Failed to disable raw mode")?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen).context("Failed to leave alternate screen")?;
        self.terminal.show_cursor().context("Failed to show cursor")?;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Try to restore terminal state on drop
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
