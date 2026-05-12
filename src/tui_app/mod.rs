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

use std::io;
use std::time::Instant;

use crossterm::event::{self, Event as CtEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// Run a TUI application: enter alternate screen + raw mode, drive the main
/// loop, then restore the terminal on exit (and on panic).
pub fn run_app<A: App>(mut app: A) -> anyhow::Result<()> {
    install_panic_hook();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let loop_result = main_loop(&mut terminal, &mut app);

    // Always restore the terminal, even if the loop errored.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    loop_result
}

fn main_loop<B: ratatui::backend::Backend, A: App>(
    terminal: &mut Terminal<B>,
    app: &mut A,
) -> anyhow::Result<()> {
    // Poll budget: tick interval if set, otherwise a reasonable idle timeout so
    // we still respond to keys quickly when ticks are off.
    let tick_rate = app.tick_interval().unwrap_or(Duration::from_millis(250));
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| app.render(f))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            match event::read()? {
                CtEvent::Key(k) => app.on_event(Event::Key(k))?,
                CtEvent::Resize(w, h) => app.on_event(Event::Resize(w, h))?,
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            if app.tick_interval().is_some() {
                app.on_event(Event::Tick)?;
            }
            last_tick = Instant::now();
        }

        if app.should_quit() {
            return Ok(());
        }
    }
}

fn install_panic_hook() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            original(info);
        }));
    });
}
