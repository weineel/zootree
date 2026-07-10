use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::time::Duration;
use zootree::tui_app::prompt::{MultiSelectPromptState, SelectPromptState};
use zootree::tui_app::{App, Event, InlineApp};

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
            Event::Paste(_) => self.last_seen = Some("paste"),
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

fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn render_inline_to_string<A: InlineApp>(app: &mut A, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| <A as InlineApp>::render(app, frame))
        .unwrap();
    buffer_to_string(terminal.backend().buffer())
}

fn down() -> KeyEvent {
    KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
}

fn numbered_items() -> Vec<String> {
    (0..12).map(|idx| format!("item-{idx:02}")).collect()
}

#[test]
fn select_render_scrolls_to_keep_cursor_visible_past_eight_items() {
    let mut app = SelectPromptState::new("Pick", numbered_items());
    for _ in 0..9 {
        app.handle_key(down());
    }

    let out = render_inline_to_string(&mut app, 40, 10);

    assert!(out.contains("> item-09"), "cursor row not visible:\n{out}");
    assert!(!out.contains("item-00"), "list did not scroll:\n{out}");
}

#[test]
fn multiselect_render_scrolls_to_keep_cursor_visible_past_eight_items() {
    let mut app = MultiSelectPromptState::new("Pick", numbered_items());
    for _ in 0..9 {
        app.handle_key(down());
    }

    let out = render_inline_to_string(&mut app, 40, 10);

    assert!(
        out.contains("> [ ] item-09"),
        "cursor row not visible:\n{out}"
    );
    assert!(!out.contains("item-00"), "list did not scroll:\n{out}");
}
