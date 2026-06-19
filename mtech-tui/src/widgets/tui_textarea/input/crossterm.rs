use super::{Input, Key};
// Use ratatui's re-exported crossterm to ensure version compatibility
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};

impl From<Event> for Input {
    /// Convert [`crossterm::event::Event`] into [`Input`].
    fn from(event: Event) -> Self {
        match event {
            Event::Key(key) => Self::from(key),
            Event::Mouse(mouse) => Self::from(mouse),
            _ => Self::default(),
        }
    }
}

impl From<KeyCode> for Key {
    /// Convert [`crossterm::event::KeyCode`] into [`Key`].
    fn from(code: KeyCode) -> Self {
        match code {
            KeyCode::Char(c) => Key::Char(c),
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Enter => Key::Enter,
            KeyCode::Left => Key::Left,
            KeyCode::Right => Key::Right,
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,
            KeyCode::Tab => Key::Tab,
            KeyCode::Delete => Key::Delete,
            KeyCode::Home => Key::Home,
            KeyCode::End => Key::End,
            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,
            KeyCode::Esc => Key::Esc,
            KeyCode::F(x) => Key::F(x),
            _ => Key::Null,
        }
    }
}

impl From<KeyEvent> for Input {
    /// Convert [`crossterm::event::KeyEvent`] into [`Input`].
    fn from(key: KeyEvent) -> Self {
        if key.kind == KeyEventKind::Release {
            // On Windows or when `crossterm::event::PushKeyboardEnhancementFlags` is set,
            // key release event can be reported. Ignore it. (#14)
            return Self::default();
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let key = Key::from(key.code);

        Self {
            key,
            ctrl,
            alt,
            shift,
        }
    }
}

impl From<MouseEventKind> for Key {
    /// Convert [`crossterm::event::MouseEventKind`] into [`Key`].
    fn from(kind: MouseEventKind) -> Self {
        match kind {
            MouseEventKind::ScrollDown => Key::MouseScrollDown,
            MouseEventKind::ScrollUp => Key::MouseScrollUp,
            _ => Key::Null,
        }
    }
}

impl From<MouseEvent> for Input {
    /// Convert [`crossterm::event::MouseEvent`] into [`Input`].
    fn from(mouse: MouseEvent) -> Self {
        let key = Key::from(mouse.kind);
        let ctrl = mouse.modifiers.contains(KeyModifiers::CONTROL);
        let alt = mouse.modifiers.contains(KeyModifiers::ALT);
        let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
        Self {
            key,
            ctrl,
            alt,
            shift,
        }
    }
}