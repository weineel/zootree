//! Inline prompts replacing the previous `dialoguer`-based `src/tui.rs`
//! implementation. Each prompt has a pure-logic state struct that handles
//! events and exposes outcome / current text / selection; rendering and
//! terminal IO are delegated to `run_inline`.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tui_textarea::{CursorMove, TextArea};

use crate::tui_app::PromptOutcome;

/// Multi-line text prompt. Backed by `tui_textarea::TextArea` for correct
/// CJK / unicode-width behavior.
pub struct TextPromptState {
    prompt: String,
    required: bool,
    textarea: TextArea<'static>,
    outcome: Option<PromptOutcome<String>>,
}

impl TextPromptState {
    pub fn new(prompt: &str) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_cursor_line_style(Style::default());
        Self {
            prompt: prompt.to_string(),
            required: true,
            textarea,
            outcome: None,
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn outcome(&self) -> Option<&PromptOutcome<String>> {
        self.outcome.as_ref()
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn is_required(&self) -> bool {
        self.required
    }

    pub fn line_count(&self) -> usize {
        self.textarea.lines().len()
    }

    /// Read-only access for rendering. The InlineApp impl in a later task
    /// will clone the textarea before applying a Block to it.
    pub fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        let alt = m.contains(KeyModifiers::ALT);
        let shift = m.contains(KeyModifiers::SHIFT);

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
            }
            KeyCode::Esc => {
                self.outcome = Some(if self.required {
                    PromptOutcome::Aborted
                } else {
                    PromptOutcome::Skipped
                });
            }
            KeyCode::Enter if alt || shift => {
                self.textarea.insert_newline();
            }
            KeyCode::Enter => {
                let text = self.text();
                self.outcome = Some(PromptOutcome::Submitted(text));
            }
            KeyCode::Backspace => {
                self.textarea.delete_char();
            }
            KeyCode::Char(c) => {
                self.textarea.insert_char(c);
            }
            KeyCode::Left => {
                self.textarea.move_cursor(CursorMove::Back);
            }
            KeyCode::Right => {
                self.textarea.move_cursor(CursorMove::Forward);
            }
            KeyCode::Up => {
                self.textarea.move_cursor(CursorMove::Up);
            }
            KeyCode::Down => {
                self.textarea.move_cursor(CursorMove::Down);
            }
            KeyCode::Home => {
                self.textarea.move_cursor(CursorMove::Head);
            }
            KeyCode::End => {
                self.textarea.move_cursor(CursorMove::End);
            }
            _ => {}
        }
    }

    pub fn handle_paste(&mut self, s: &str) {
        if self.outcome.is_some() {
            return;
        }
        self.textarea.insert_str(s);
    }
}

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

/// Select-one prompt with always-on fuzzy filter. Pure logic; rendering and
/// terminal IO are handled by the `InlineApp` impl in a later task.
pub struct SelectPromptState {
    prompt: String,
    items: Vec<String>,
    filter: TextArea<'static>,
    visible: Vec<usize>,
    cursor: usize,
    matcher: SkimMatcherV2,
    outcome: Option<PromptOutcome<usize>>,
}

impl SelectPromptState {
    pub fn new(prompt: &str, items: Vec<String>) -> Self {
        let visible = (0..items.len()).collect();
        let mut filter = TextArea::default();
        filter.set_cursor_line_style(Style::default());
        Self {
            prompt: prompt.to_string(),
            items,
            filter,
            visible,
            cursor: 0,
            matcher: SkimMatcherV2::default(),
            outcome: None,
        }
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }
    pub fn items(&self) -> &[String] {
        &self.items
    }
    pub fn visible_indices(&self) -> Vec<usize> {
        self.visible.clone()
    }
    pub fn cursor_visible_index(&self) -> Option<usize> {
        if self.visible.is_empty() {
            None
        } else {
            Some(self.cursor.min(self.visible.len() - 1))
        }
    }
    pub fn filter_text(&self) -> String {
        self.filter.lines().join("")
    }
    pub fn outcome(&self) -> Option<&PromptOutcome<usize>> {
        self.outcome.as_ref()
    }

    /// Character indices of the fuzzy match for `items[item_idx]` against the
    /// current filter, suitable for highlighting individual chars in the
    /// renderer. Returns char indices (NOT byte indices), so `Style` spans
    /// over a Chinese character get applied correctly.
    pub fn match_indices_for(&self, item_idx: usize) -> Vec<usize> {
        let f = self.filter_text();
        if f.is_empty() {
            return Vec::new();
        }
        let item = &self.items[item_idx];
        let byte_hits = self
            .matcher
            .fuzzy_indices(item, &f)
            .map(|(_score, idxs)| idxs)
            .unwrap_or_default();
        let byte_to_char: std::collections::HashMap<usize, usize> = item
            .char_indices()
            .enumerate()
            .map(|(ci, (bi, _))| (bi, ci))
            .collect();
        byte_hits
            .into_iter()
            .filter_map(|b| byte_to_char.get(&b).copied())
            .collect()
    }

    fn recompute_visible(&mut self) {
        let f = self.filter_text();
        if f.is_empty() {
            self.visible = (0..self.items.len()).collect();
        } else {
            let mut scored: Vec<(i64, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| self.matcher.fuzzy_match(item, &f).map(|s| (s, i)))
                .collect();
            scored.sort_by_key(|b| std::cmp::Reverse(b.0));
            self.visible = scored.into_iter().map(|(_, i)| i).collect();
        }
        if !self.visible.is_empty() && self.cursor >= self.visible.len() {
            self.cursor = self.visible.len() - 1;
        } else if self.visible.is_empty() {
            self.cursor = 0;
        }
    }

    pub(crate) fn clear_outcome_if_set(&mut self) {
        // Used by `MultiSelectPromptState` (Task 5) which embeds a select
        // state purely for filter + navigation; we don't want the inner
        // state's outcome to leak.
        self.outcome = None;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
            }
            KeyCode::Esc => {
                self.outcome = Some(PromptOutcome::Aborted);
            }
            KeyCode::Enter => {
                if let Some(vi) = self.cursor_visible_index() {
                    let original = self.visible[vi];
                    self.outcome = Some(PromptOutcome::Submitted(original));
                }
            }
            KeyCode::Down if !self.visible.is_empty() => {
                self.cursor = (self.cursor + 1) % self.visible.len();
            }
            KeyCode::Char('n') if ctrl && !self.visible.is_empty() => {
                self.cursor = (self.cursor + 1) % self.visible.len();
            }
            KeyCode::Up if !self.visible.is_empty() => {
                self.cursor = if self.cursor == 0 {
                    self.visible.len() - 1
                } else {
                    self.cursor - 1
                };
            }
            KeyCode::Char('p') if ctrl && !self.visible.is_empty() => {
                self.cursor = if self.cursor == 0 {
                    self.visible.len() - 1
                } else {
                    self.cursor - 1
                };
            }
            KeyCode::Backspace => {
                self.filter.delete_char();
                self.recompute_visible();
            }
            KeyCode::Char(c) => {
                self.filter.insert_char(c);
                self.recompute_visible();
            }
            _ => {}
        }
    }
}

/// Select-many prompt: same filter/navigation as `SelectPromptState`, plus a
/// per-item checked vector. Submit returns indices in **selection order**
/// (matches dialoguer's `MultiSelect` behavior to keep the existing 9
/// callsites' contract).
pub struct MultiSelectPromptState {
    inner: SelectPromptState,
    selection_order: Vec<usize>,
    checked: Vec<bool>,
    outcome: Option<PromptOutcome<Vec<usize>>>,
}

impl MultiSelectPromptState {
    pub fn new(prompt: &str, items: Vec<String>) -> Self {
        let n = items.len();
        Self {
            inner: SelectPromptState::new(prompt, items),
            selection_order: Vec::new(),
            checked: vec![false; n],
            outcome: None,
        }
    }

    pub fn with_defaults(prompt: &str, items: Vec<String>, default_indices: &[usize]) -> Self {
        let mut state = Self::new(prompt, items);
        for &idx in default_indices {
            if idx < state.checked.len() && !state.checked[idx] {
                state.checked[idx] = true;
                state.selection_order.push(idx);
            }
        }
        state
    }

    pub fn prompt(&self) -> &str {
        self.inner.prompt()
    }
    pub fn items(&self) -> &[String] {
        self.inner.items()
    }
    pub fn visible_indices(&self) -> Vec<usize> {
        self.inner.visible_indices()
    }
    pub fn cursor_visible_index(&self) -> Option<usize> {
        self.inner.cursor_visible_index()
    }
    pub fn filter_text(&self) -> String {
        self.inner.filter_text()
    }
    pub fn is_checked(&self, original_idx: usize) -> bool {
        self.checked.get(original_idx).copied().unwrap_or(false)
    }
    pub fn match_indices_for(&self, item_idx: usize) -> Vec<usize> {
        self.inner.match_indices_for(item_idx)
    }
    pub fn outcome(&self) -> Option<&PromptOutcome<Vec<usize>>> {
        self.outcome.as_ref()
    }

    fn toggle(&mut self, original_idx: usize) {
        if self.checked[original_idx] {
            self.checked[original_idx] = false;
            self.selection_order.retain(|&i| i != original_idx);
        } else {
            self.checked[original_idx] = true;
            self.selection_order.push(original_idx);
        }
    }

    fn select_all_toggle(&mut self) {
        let all_set = self.checked.iter().all(|&c| c);
        if all_set {
            self.checked.fill(false);
            self.selection_order.clear();
        } else {
            for (i, c) in self.checked.iter_mut().enumerate() {
                if !*c {
                    *c = true;
                    self.selection_order.push(i);
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
            }
            KeyCode::Esc => {
                self.outcome = Some(PromptOutcome::Aborted);
            }
            KeyCode::Char(' ') => {
                if let Some(vi) = self.inner.cursor_visible_index() {
                    let original = self.inner.visible_indices()[vi];
                    self.toggle(original);
                }
            }
            KeyCode::Char('a') if !ctrl && self.inner.filter_text().is_empty() => {
                // 'a' is the select-all toggle ONLY when the filter is empty;
                // otherwise it must continue into the filter as a literal char.
                self.select_all_toggle();
            }
            KeyCode::Enter => {
                self.outcome = Some(PromptOutcome::Submitted(self.selection_order.clone()));
            }
            _ => {
                // Delegate movement / filter / typing to the inner SelectPromptState.
                // The inner may set its own outcome for keys we don't intercept
                // (Esc/Ctrl+C are already covered above; this defensive sweep
                // ensures we never leak the inner outcome to callers).
                self.inner.handle_key(key);
                self.inner.clear_outcome_if_set();
            }
        }
    }
}

/// Yes/No confirmation prompt. Single-line; Enter accepts the default value.
pub struct ConfirmPromptState {
    prompt: String,
    default: bool,
    outcome: Option<PromptOutcome<bool>>,
}

impl ConfirmPromptState {
    pub fn new(prompt: &str, default: bool) -> Self {
        Self {
            prompt: prompt.to_string(),
            default,
            outcome: None,
        }
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }
    pub fn default_value(&self) -> bool {
        self.default
    }
    pub fn outcome(&self) -> Option<&PromptOutcome<bool>> {
        self.outcome.as_ref()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.outcome.is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('c') if ctrl => {
                self.outcome = Some(PromptOutcome::Interrupted);
            }
            KeyCode::Esc => {
                self.outcome = Some(PromptOutcome::Aborted);
            }
            KeyCode::Enter => {
                self.outcome = Some(PromptOutcome::Submitted(self.default));
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.outcome = Some(PromptOutcome::Submitted(true));
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.outcome = Some(PromptOutcome::Submitted(false));
            }
            _ => {}
        }
    }
}

use crate::tui_app::{Event, InlineApp};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

const TEXT_MIN_VISIBLE_LINES: u16 = 5;
const TEXT_MAX_VISIBLE_LINES: u16 = 10;

impl InlineApp for TextPromptState {
    type Output = String;

    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        match event {
            Event::Key(k) => self.handle_key(k),
            Event::Paste(s) => self.handle_paste(&s),
            Event::Resize(_, _) | Event::Tick => {}
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // header
                Constraint::Min(1),    // editor
                Constraint::Length(1), // help
            ])
            .split(area);

        let header_spans = if self.is_required() {
            vec![Span::styled(
                format!("> {}", self.prompt()),
                Style::default().fg(Color::Cyan),
            )]
        } else {
            vec![
                Span::styled(
                    format!("> {}", self.prompt()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" "),
                Span::styled("(optional)", Style::default().fg(Color::DarkGray)),
            ]
        };
        frame.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

        let mut ta = self.textarea().clone();
        ta.set_block(Block::default().borders(Borders::ALL));
        frame.render_widget(&ta, chunks[1]);

        let help = if self.is_required() {
            "enter submit · alt+enter newline · esc cancel · ctrl+c abort"
        } else {
            "enter submit · alt+enter newline · esc skip · ctrl+c abort"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(help, Style::default().fg(Color::DarkGray))),
            chunks[2],
        );
    }

    fn desired_height(&self) -> u16 {
        let editor =
            (self.line_count() as u16).clamp(TEXT_MIN_VISIBLE_LINES, TEXT_MAX_VISIBLE_LINES);
        // 1 header + editor + 2 borders + 1 help
        1 + editor + 2 + 1
    }

    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        self.outcome.clone()
    }

    fn summary(&self) -> Option<String> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(text)) => {
                let first = text.lines().next().unwrap_or("");
                let suffix = if text.lines().count() > 1 { " …" } else { "" };
                Some(format!("✔ {}: {}{}", self.prompt(), first, suffix))
            }
            _ => None,
        }
    }
}

const SELECT_MAX_VISIBLE_ROWS: u16 = 8;

impl InlineApp for SelectPromptState {
    type Output = usize;

    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        if let Event::Key(k) = event {
            self.handle_key(k);
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // prompt + filter
                Constraint::Min(1),    // list
                Constraint::Length(1), // help
            ])
            .split(area);

        let header_spans = vec![
            Span::styled(
                format!("> {}: ", self.prompt()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(self.filter_text()),
            Span::styled("█", Style::default().fg(Color::DarkGray)),
        ];
        frame.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

        let visible = self.visible_indices();
        let cursor = self.cursor_visible_index();
        let lines: Vec<Line> = visible
            .iter()
            .enumerate()
            .take(SELECT_MAX_VISIBLE_ROWS as usize)
            .map(|(vi, &orig)| {
                let item = &self.items()[orig];
                let hits = self.match_indices_for(orig);
                let mut spans: Vec<Span> = Vec::new();
                let style = if Some(vi) == cursor {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                let prefix = if Some(vi) == cursor { "> " } else { "  " };
                spans.push(Span::styled(prefix, style));
                for (i, ch) in item.chars().enumerate() {
                    let mut s = style;
                    if hits.contains(&i) {
                        s = s.fg(Color::Yellow);
                    }
                    spans.push(Span::styled(ch.to_string(), s));
                }
                Line::from(spans)
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), chunks[1]);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "↑↓ navigate · enter select · esc cancel · ctrl+c abort",
                Style::default().fg(Color::DarkGray),
            )),
            chunks[2],
        );
    }

    fn desired_height(&self) -> u16 {
        let rows = (self.visible_indices().len() as u16).clamp(1, SELECT_MAX_VISIBLE_ROWS);
        1 + rows + 1
    }

    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        self.outcome.clone()
    }

    fn summary(&self) -> Option<String> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(idx)) => {
                Some(format!("✔ {}: {}", self.prompt(), self.items()[*idx]))
            }
            _ => None,
        }
    }
}

impl InlineApp for MultiSelectPromptState {
    type Output = Vec<usize>;

    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        if let Event::Key(k) = event {
            self.handle_key(k);
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        let header_spans = vec![
            Span::styled(
                format!("> {}: ", self.prompt()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(self.filter_text()),
            Span::styled("█", Style::default().fg(Color::DarkGray)),
        ];
        frame.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

        let visible = self.visible_indices();
        let cursor = self.cursor_visible_index();
        let lines: Vec<Line> = visible
            .iter()
            .enumerate()
            .take(SELECT_MAX_VISIBLE_ROWS as usize)
            .map(|(vi, &orig)| {
                let item = &self.items()[orig];
                let hits = self.match_indices_for(orig);
                let style = if Some(vi) == cursor {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                let cursor_prefix = if Some(vi) == cursor { ">" } else { " " };
                let check = if self.is_checked(orig) { "[x]" } else { "[ ]" };
                let mut spans = vec![Span::styled(format!("{} {} ", cursor_prefix, check), style)];
                for (i, ch) in item.chars().enumerate() {
                    let mut s = style;
                    if hits.contains(&i) {
                        s = s.fg(Color::Yellow);
                    }
                    spans.push(Span::styled(ch.to_string(), s));
                }
                Line::from(spans)
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), chunks[1]);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "↑↓ navigate · space toggle · a all · enter submit · esc cancel · ctrl+c abort",
                Style::default().fg(Color::DarkGray),
            )),
            chunks[2],
        );
    }

    fn desired_height(&self) -> u16 {
        let rows = (self.visible_indices().len() as u16).clamp(1, SELECT_MAX_VISIBLE_ROWS);
        1 + rows + 1
    }

    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        self.outcome.clone()
    }

    fn summary(&self) -> Option<String> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(v)) => {
                Some(format!("✔ {}: {} selected", self.prompt(), v.len()))
            }
            _ => None,
        }
    }
}

impl InlineApp for ConfirmPromptState {
    type Output = bool;

    fn on_event(&mut self, event: Event) -> anyhow::Result<()> {
        if let Event::Key(k) = event {
            self.handle_key(k);
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let hint = if self.default_value() {
            "[Y/n]"
        } else {
            "[y/N]"
        };
        let line = Line::from(vec![
            Span::styled(
                format!("> {} ", self.prompt()),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(hint, Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn desired_height(&self) -> u16 {
        1
    }

    fn poll(&mut self) -> Option<PromptOutcome<Self::Output>> {
        self.outcome.clone()
    }

    fn summary(&self) -> Option<String> {
        match &self.outcome {
            Some(PromptOutcome::Submitted(v)) => Some(format!(
                "✔ {}: {}",
                self.prompt(),
                if *v { "yes" } else { "no" }
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui_app::{InlineApp, PromptOutcome};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn key_mod(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn text_cjk_backspace_removes_one_char() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('你')));
        s.handle_key(key(KeyCode::Char('好')));
        assert_eq!(s.text(), "你好");
        s.handle_key(key(KeyCode::Backspace));
        assert_eq!(s.text(), "你");
    }

    #[test]
    fn text_alt_enter_inserts_newline() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_key(key(KeyCode::Char('a')));
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        s.handle_key(key(KeyCode::Char('b')));
        assert_eq!(s.text(), "a\nb");
        assert!(s.outcome().is_none());
    }

    #[test]
    fn text_shift_enter_inserts_newline() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_key(key(KeyCode::Char('a')));
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::SHIFT));
        s.handle_key(key(KeyCode::Char('b')));
        assert_eq!(s.text(), "a\nb");
    }

    #[test]
    fn text_enter_submits() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('x')));
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(t)) if t == "x"));
    }

    #[test]
    fn text_optional_esc_skipped() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Skipped)));
    }

    #[test]
    fn text_required_esc_aborted() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }

    #[test]
    fn text_ctrl_c_interrupted() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Interrupted)));
    }

    #[test]
    fn text_paste_inserts_multiline_without_submitting() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_paste("line1\nline2");
        assert_eq!(s.text(), "line1\nline2");
        assert!(s.outcome().is_none());
    }

    #[test]
    fn text_after_outcome_keys_are_ignored() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Esc));
        s.handle_key(key(KeyCode::Char('a')));
        assert_eq!(s.text(), "");
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }

    #[test]
    fn text_paste_crlf_inserts_single_newline() {
        let mut s = TextPromptState::new("Desc").optional();
        s.handle_paste("line1\r\nline2");
        assert_eq!(s.text(), "line1\nline2");
    }

    #[test]
    fn text_char_c_without_ctrl_inserts_literal() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('c')));
        assert_eq!(s.text(), "c");
        assert!(s.outcome().is_none());
    }

    #[test]
    fn select_filter_cjk_filters_items() {
        let items = vec!["前端".to_string(), "后端".to_string(), "中间件".to_string()];
        let mut s = SelectPromptState::new("Choose", items);
        s.handle_key(key(KeyCode::Char('前')));
        let visible = s.visible_indices();
        assert_eq!(visible, vec![0]);
    }

    #[test]
    fn select_arrow_navigation_wraps() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut s = SelectPromptState::new("Pick", items);
        assert_eq!(s.cursor_visible_index(), Some(0));
        s.handle_key(key(KeyCode::Down));
        assert_eq!(s.cursor_visible_index(), Some(1));
        s.handle_key(key(KeyCode::Down));
        s.handle_key(key(KeyCode::Down));
        assert_eq!(s.cursor_visible_index(), Some(0));
        s.handle_key(key(KeyCode::Up));
        assert_eq!(s.cursor_visible_index(), Some(2));
    }

    #[test]
    fn select_enter_submits_original_index() {
        let items = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key(KeyCode::Down));
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(1))));
    }

    #[test]
    fn select_enter_after_filter_returns_original_index() {
        let items = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key(KeyCode::Char('g')));
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(2))));
    }

    #[test]
    fn select_enter_with_empty_filter_match_is_noop() {
        let items = vec!["alpha".into(), "beta".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key(KeyCode::Char('z')));
        s.handle_key(key(KeyCode::Char('z')));
        s.handle_key(key(KeyCode::Char('z')));
        assert!(s.visible_indices().is_empty());
        s.handle_key(key(KeyCode::Enter));
        assert!(s.outcome().is_none());
    }

    #[test]
    fn select_esc_aborted() {
        let items = vec!["a".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }

    #[test]
    fn select_ctrl_c_interrupted() {
        let items = vec!["a".into()];
        let mut s = SelectPromptState::new("Pick", items);
        s.handle_key(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Interrupted)));
    }

    #[test]
    fn multi_space_toggles_current_item() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into()]);
        s.handle_key(key(KeyCode::Char(' ')));
        assert!(s.is_checked(0));
        s.handle_key(key(KeyCode::Char(' ')));
        assert!(!s.is_checked(0));
    }

    #[test]
    fn multi_a_toggles_select_all() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into(), "c".into()]);
        s.handle_key(key(KeyCode::Char('a')));
        assert!(s.is_checked(0) && s.is_checked(1) && s.is_checked(2));
        s.handle_key(key(KeyCode::Char('a')));
        assert!(!s.is_checked(0) && !s.is_checked(1) && !s.is_checked(2));
    }

    #[test]
    fn multi_returns_indices_in_selection_order() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into(), "c".into()]);
        s.handle_key(key(KeyCode::Down));
        s.handle_key(key(KeyCode::Down));
        s.handle_key(key(KeyCode::Char(' '))); // selects "c" first
        s.handle_key(key(KeyCode::Up));
        s.handle_key(key(KeyCode::Up));
        s.handle_key(key(KeyCode::Char(' '))); // selects "a" second
        s.handle_key(key(KeyCode::Enter));
        match s.outcome() {
            Some(PromptOutcome::Submitted(v)) => assert_eq!(v, &vec![2, 0]),
            other => panic!("expected Submitted([2, 0]), got {:?}", other.is_some()),
        }
    }

    #[test]
    fn multi_enter_with_no_selection_submits_empty() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into()]);
        s.handle_key(key(KeyCode::Enter));
        match s.outcome() {
            Some(PromptOutcome::Submitted(v)) => assert!(v.is_empty()),
            _ => panic!("expected Submitted([])"),
        }
    }

    #[test]
    fn multi_with_defaults_preselects_items() {
        let mut s =
            MultiSelectPromptState::with_defaults("Pick", vec!["a".into(), "b".into()], &[1]);
        assert!(!s.is_checked(0));
        assert!(s.is_checked(1));
        s.handle_key(key(KeyCode::Enter));
        match s.outcome() {
            Some(PromptOutcome::Submitted(v)) => assert_eq!(v, &vec![1]),
            _ => panic!("expected Submitted([1])"),
        }
    }

    #[test]
    fn multi_filter_then_toggle_then_clear_filter_keeps_selection() {
        let mut s = MultiSelectPromptState::new(
            "Pick",
            vec!["alpha".into(), "beta".into(), "gamma".into()],
        );
        // type 'g' -> only gamma visible at visible[0]
        s.handle_key(key(KeyCode::Char('g')));
        s.handle_key(key(KeyCode::Char(' ')));
        assert!(s.is_checked(2));
        // backspace -> all visible again
        s.handle_key(key(KeyCode::Backspace));
        assert_eq!(s.visible_indices(), vec![0, 1, 2]);
        assert!(s.is_checked(2));
    }

    #[test]
    fn multi_esc_aborted() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into()]);
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }

    #[test]
    fn confirm_default_true_on_enter_returns_true() {
        let mut s = ConfirmPromptState::new("Delete?", true);
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(true))));
    }

    #[test]
    fn confirm_default_false_on_enter_returns_false() {
        let mut s = ConfirmPromptState::new("Delete?", false);
        s.handle_key(key(KeyCode::Enter));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(false))));
    }

    #[test]
    fn confirm_y_returns_true() {
        let mut s = ConfirmPromptState::new("Delete?", false);
        s.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(true))));
    }

    #[test]
    fn confirm_capital_n_returns_false() {
        let mut s = ConfirmPromptState::new("Delete?", true);
        s.handle_key(key(KeyCode::Char('N')));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Submitted(false))));
    }

    #[test]
    fn confirm_esc_aborted() {
        let mut s = ConfirmPromptState::new("Delete?", false);
        s.handle_key(key(KeyCode::Esc));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Aborted)));
    }

    #[test]
    fn confirm_ctrl_c_interrupted() {
        let mut s = ConfirmPromptState::new("Delete?", true);
        s.handle_key(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(s.outcome(), Some(PromptOutcome::Interrupted)));
    }

    #[test]
    fn text_desired_height_floor_is_min_visible() {
        let s = TextPromptState::new("Title").required();
        // 1 header + 5 editor (floor) + 2 borders + 1 help = 9
        assert_eq!(s.desired_height(), 9);
    }

    #[test]
    fn text_desired_height_unchanged_for_few_newlines() {
        let mut s = TextPromptState::new("Desc").optional();
        // Three Alt+Enter → line_count = 4, still under floor 5
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        assert_eq!(s.line_count(), 4);
        assert_eq!(s.desired_height(), 9);
    }

    #[test]
    fn text_desired_height_grows_above_floor() {
        let mut s = TextPromptState::new("Desc").optional();
        for _ in 0..6 {
            s.handle_key(key_mod(KeyCode::Enter, KeyModifiers::ALT));
        }
        // line_count = 7 → 1 + 7 + 2 + 1 = 11
        assert_eq!(s.line_count(), 7);
        assert_eq!(s.desired_height(), 11);
    }

    fn key_release(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        use crossterm::event::KeyEventKind;
        KeyEvent::new_with_kind(code, m, KeyEventKind::Release)
    }

    #[test]
    fn text_release_event_is_ignored() {
        let mut s = TextPromptState::new("Title").required();
        s.handle_key(key(KeyCode::Char('a')));
        s.handle_key(key_release(KeyCode::Enter, KeyModifiers::ALT));
        assert_eq!(s.text(), "a");
        assert_eq!(s.line_count(), 1);
        assert!(s.outcome().is_none());
    }

    #[test]
    fn select_release_event_is_ignored() {
        let items = vec!["a".into(), "b".into(), "c".into()];
        let mut s = SelectPromptState::new("Pick", items);
        // cursor starts at 0; a Release Down event must NOT advance it.
        s.handle_key(key_release(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(s.cursor_visible_index(), Some(0));
        assert!(s.filter_text().is_empty());
    }

    #[test]
    fn multi_release_event_is_ignored() {
        let mut s = MultiSelectPromptState::new("Pick", vec!["a".into(), "b".into()]);
        s.handle_key(key_release(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!s.is_checked(0));
        assert!(s.outcome().is_none());
    }

    #[test]
    fn confirm_release_event_is_ignored() {
        let mut s = ConfirmPromptState::new("Delete?", false);
        s.handle_key(key_release(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(s.outcome().is_none());
    }
}
