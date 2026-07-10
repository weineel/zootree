use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui_textarea::{CursorMove, TextArea, WrapMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WizardTextKind {
    SingleLine,
    Multiline,
}

pub(super) struct WizardTextField {
    pub(super) textarea: TextArea<'static>,
}

impl WizardTextField {
    pub(super) fn new(text: impl Into<String>, _kind: WizardTextKind) -> Self {
        let text = text.into();
        let mut textarea = if text.is_empty() {
            TextArea::default()
        } else {
            TextArea::from(text.split('\n').map(str::to_string).collect::<Vec<_>>())
        };
        textarea.move_cursor(CursorMove::Bottom);
        textarea.move_cursor(CursorMove::End);
        textarea.set_cursor_line_style(Style::default());
        textarea.set_wrap_mode(WrapMode::WordOrGlyph);
        Self { textarea }
    }

    pub(super) fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('u') if ctrl => {
                let _ = self.textarea.delete_line_by_head();
            }
            KeyCode::Enter if alt || shift => self.textarea.insert_newline(),
            _ => {
                let _ = self.textarea.input(key);
            }
        }
    }

    pub(super) fn handle_paste(&mut self, text: &str) {
        self.textarea.insert_str(text);
    }
}
