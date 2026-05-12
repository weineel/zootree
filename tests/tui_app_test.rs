use std::time::Duration;
use zootree::tui_app::{App, Event};

struct NoopApp {
    quit: bool,
    last_seen: Option<&'static str>,
}

impl NoopApp {
    fn new() -> Self {
        Self {
            quit: false,
            last_seen: None,
        }
    }
}

impl App for NoopApp {
    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        match event {
            Event::Key(_) => self.last_seen = Some("key"),
            Event::Tick => self.last_seen = Some("tick"),
            Event::Resize(_, _) => self.last_seen = Some("resize"),
        }
        self.quit = true;
        Ok(())
    }
    fn render(&mut self, _frame: &mut ratatui::Frame) {}
    fn should_quit(&self) -> bool {
        self.quit
    }
}

#[test]
fn default_tick_interval_is_none() {
    let app = NoopApp::new();
    assert_eq!(app.tick_interval(), None);
}

#[test]
fn key_event_can_be_dispatched() {
    let mut app = NoopApp::new();
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::NONE,
    );
    app.on_event(Event::Key(key)).unwrap();
    assert_eq!(app.last_seen, Some("key"));
    assert!(app.should_quit());
}

#[test]
fn tick_event_can_be_dispatched() {
    let mut app = NoopApp::new();
    app.on_event(Event::Tick).unwrap();
    assert_eq!(app.last_seen, Some("tick"));
}

#[test]
fn resize_event_carries_dimensions() {
    let mut app = NoopApp::new();
    app.on_event(Event::Resize(80, 24)).unwrap();
    assert_eq!(app.last_seen, Some("resize"));
}

#[test]
fn custom_tick_interval_overrides_default() {
    struct WatchApp;
    impl App for WatchApp {
        fn on_event(&mut self, _: Event) -> anyhow::Result<()> {
            Ok(())
        }
        fn render(&mut self, _: &mut ratatui::Frame) {}
        fn should_quit(&self) -> bool {
            false
        }
        fn tick_interval(&self) -> Option<Duration> {
            Some(Duration::from_secs(2))
        }
    }
    assert_eq!(WatchApp.tick_interval(), Some(Duration::from_secs(2)));
}
