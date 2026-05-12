//! TUI application framework built on ratatui + crossterm.
//!
//! Each interactive view implements the `App` trait; `run_app` drives the
//! terminal setup, event loop, and cleanup.

use std::time::Duration;

pub mod info;

/// Events dispatched to the active `App` on every iteration of the main loop.
pub enum Event {
    Key(crossterm::event::KeyEvent),
    Tick,
    Resize(u16, u16),
}

/// A TUI view. Implementors own their state, decide when to quit, and render
/// one frame at a time.
pub trait App {
    fn on_event(&mut self, event: Event) -> anyhow::Result<()>;
    fn render(&mut self, frame: &mut ratatui::Frame);
    fn should_quit(&self) -> bool;

    /// Override to request periodic `Event::Tick` delivery. `None` disables
    /// ticks (the loop still polls for key/resize events).
    fn tick_interval(&self) -> Option<Duration> {
        None
    }
}
