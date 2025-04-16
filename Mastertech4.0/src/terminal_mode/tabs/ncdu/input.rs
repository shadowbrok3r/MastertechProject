use crossterm::event::{Event, KeyCode};

/// Input enum to represent the fours different actions available inside a [`FileExplorer`](crate::FileExplorer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    /// Move the selection up.
    Up,
    /// Move the selection down.
    Down,
    /// Select the first entry.
    Home,
    /// Select the last entry.
    End,
    /// Scroll several entries up.
    PageUp,
    /// Scroll several entries down.
    PageDown,
    /// Go to the parent directory.
    Left,
    /// Go to the child directory (if the selected item is a directory).
    Right,
    /// Do nothing (used for converting events from other libraries, like
    /// [crossterm](https://docs.rs/crossterm/latest/crossterm/event/enum.Event.html),
    /// [termion](https://docs.rs/termion/latest/termion/event/enum.Event.html) and
    /// [termwiz](https://docs.rs/termwiz/latest/termwiz/input/enum.InputEvent.html) to [`Input`]).
    None,
}

impl From<&Event> for Input {
    /// Convert crossterm [`Event`](https://docs.rs/crossterm/latest/crossterm/event/enum.Event.html) to [`Input`].
    ///
    /// **Note:** This implementation is only available when the `crossterm` feature is enabled.
    fn from(value: &Event) -> Self {
        if let Event::Key(key) = value {
            if matches!(
                key.kind,
                crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
            ) {
                let input = match key.code {
                    KeyCode::Char('j') | KeyCode::Down => Input::Down,
                    KeyCode::Char('k') | KeyCode::Up => Input::Up,
                    KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => Input::Left,
                    KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => Input::Right,
                    KeyCode::Home => Input::Home,
                    KeyCode::End => Input::End,
                    KeyCode::PageUp => Input::PageUp,
                    KeyCode::PageDown => Input::PageDown,
                    _ => Input::None,
                };

                return input;
            }
        }

        Input::None
    }
}