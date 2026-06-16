//! TUI application framework built on ratatui + crossterm.
//!
//! Each interactive view implements the `App` trait; `run_app` drives the
//! terminal setup, event loop, and cleanup.

use std::time::Duration;

pub mod info;
pub mod prompt;

/// Events dispatched to the active `App` on every iteration of the main loop.
pub enum Event {
    Key(crossterm::event::KeyEvent),
    Tick,
    Resize(u16, u16),
    Paste(String),
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

use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event as CtEvent, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
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
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
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
            // Restore terminal in the broadest order: pop kitty flags (no-op
            // if not pushed), disable bracketed paste (no-op if not enabled),
            // leave alternate screen (no-op if not entered), disable raw mode.
            // Each is best-effort; ignore errors.
            let _ = execute!(
                io::stdout(),
                PopKeyboardEnhancementFlags,
                DisableBracketedPaste,
                LeaveAlternateScreen,
            );
            let _ = disable_raw_mode();
            original(info);
        }));
    });
}

/// Outcome of an inline prompt, returned by `run_inline`.
#[derive(Clone)]
pub enum PromptOutcome<T> {
    Submitted(T),
    Skipped,
    Aborted,
    Interrupted,
}

/// Sentinel error inserted into the `anyhow` chain when an inline prompt is
/// cancelled by the user (Esc on a required prompt, or Ctrl+C anywhere).
/// `main` downcasts to this and exits cleanly with code 1.
#[derive(Debug)]
pub struct CancelledByUser;

impl std::fmt::Display for CancelledByUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "aborted")
    }
}

impl std::error::Error for CancelledByUser {}

/// Inline (non-fullscreen) TUI app. Shares the `Event` type with `App`, but
/// uses `Viewport::Inline` and exposes a typed `Output` to `run_inline`.
pub trait InlineApp {
    type Output;
    fn on_event(&mut self, event: Event) -> anyhow::Result<()>;
    fn render(&mut self, frame: &mut ratatui::Frame);
    fn desired_height(&self) -> u16;
    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>>;
    /// One-line summary written to scrollback after the prompt exits with
    /// `Submitted`. `None` => write nothing.
    fn summary(&self) -> Option<String> {
        None
    }
}

pub fn run_inline<A: InlineApp>(mut app: A) -> anyhow::Result<PromptOutcome<A::Output>> {
    install_panic_hook();

    enable_raw_mode()?;
    let mut stdout = io::stdout();

    // Best-effort: enable kitty keyboard protocol so Shift+Enter is reported
    // distinctly from Enter. Terminals that don't support it silently ignore.
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
        ),
    );

    // Bracketed paste so multi-line paste arrives as a single Event::Paste(s)
    // instead of being interpreted character-by-character.
    let _ = execute!(stdout, EnableBracketedPaste);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::with_options(
        backend,
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Inline(app.desired_height().max(1)),
        },
    )?;

    let result = inline_loop(&mut terminal, &mut app);

    // Cleanup. Each step best-effort.
    let _ = execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        PopKeyboardEnhancementFlags,
    );
    let _ = disable_raw_mode();

    // Write a one-line scrollback summary on Submitted only.
    if let Ok(PromptOutcome::Submitted(_)) = &result {
        if let Some(line) = app.summary() {
            let _ = terminal.insert_before(1, |buf| {
                let area = buf.area;
                buf.set_string(area.x, area.y, &line, ratatui::style::Style::default());
            });
        }
    }
    let _ = terminal.clear();

    result
}

fn inline_loop<B: ratatui::backend::Backend, A: InlineApp>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut A,
) -> anyhow::Result<PromptOutcome<A::Output>>
where
    B::Error: Send + Sync + 'static,
{
    let mut last_height = app.desired_height().max(1);
    loop {
        let height = app.desired_height().max(1);
        if height != last_height {
            terminal.resize(ratatui::layout::Rect::new(
                0,
                0,
                terminal.size()?.width,
                height,
            ))?;
            last_height = height;
        }
        terminal.draw(|f| app.render(f))?;

        if let Some(outcome) = app.poll() {
            return Ok(outcome);
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                CtEvent::Key(k) => app.on_event(Event::Key(k))?,
                CtEvent::Resize(w, h) => app.on_event(Event::Resize(w, h))?,
                CtEvent::Paste(s) => app.on_event(Event::Paste(s))?,
                _ => {}
            }
        }
    }
}
