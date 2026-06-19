use ratatui::style::{Color, Modifier, Style};
use super::{APP_BACKGROUND, CATPPUCCIN};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub text: Color,
    pub background: Color,
    pub highlight: Color,
    pub shadow: Color,
}

/// Unified terminal-mode palette: pink accent + purple tertiary on the dark app background.
#[allow(dead_code)]
pub struct AppTheme {
    pub bg: Color,
    pub surface: Color,
    pub accent: Color,
    pub tertiary: Color,
    pub text: Color,
    pub text_muted: Color,
    pub success: Color,
    pub error: Color,
    pub warning: Color,
}

pub static THEME: AppTheme = AppTheme {
    bg: APP_BACKGROUND,
    surface: CATPPUCCIN.surface0,
    accent: Color::Rgb(255, 20, 147),
    tertiary: CATPPUCCIN.mauve,
    text: CATPPUCCIN.text,
    text_muted: CATPPUCCIN.subtext1,
    success: CATPPUCCIN.green,
    error: CATPPUCCIN.red,
    warning: CATPPUCCIN.yellow,
};

#[allow(dead_code)]
impl AppTheme {
    /// Pink border for focused / active panels.
    pub const fn border_active(&self) -> Color {
        self.accent
    }

    /// Muted border for idle panels.
    pub const fn border_idle(&self) -> Color {
        CATPPUCCIN.surface2
    }

    /// Marker color for a checkbox/todo: pink when checked, purple when not.
    pub const fn checkbox(&self, checked: bool) -> Color {
        if checked { self.accent } else { self.tertiary }
    }

    /// Highlight style for the selected row in a menu / list.
    pub fn menu_highlight(&self) -> Style {
        Style::new().bg(self.surface).fg(self.accent).add_modifier(Modifier::BOLD)
    }

    /// Bold pink title style for panel headers.
    pub fn title(&self) -> Style {
        Style::new().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    /// Border style for a panel, pink when focused, muted otherwise.
    pub fn border(&self, focused: bool) -> Style {
        Style::new().fg(if focused { self.border_active() } else { self.border_idle() })
    }
}

impl Theme {
    /// Pink primary-action theme (Run, Submit).
    pub const ACCENT: Theme = Theme {
        text: CATPPUCCIN.text,
        background: Color::Rgb(255, 20, 147),
        highlight: Color::Rgb(255, 20, 147),
        shadow: Color::Rgb(127, 10, 73),
    };

    /// Muted neutral theme: grey border, no rainbow.
    pub const NEUTRAL: Theme = Theme {
        text: CATPPUCCIN.subtext1,
        background: CATPPUCCIN.surface2,
        highlight: CATPPUCCIN.surface2,
        shadow: CATPPUCCIN.surface0,
    };

    /// Purple secondary-control theme.
    pub const TERTIARY: Theme = Theme {
        text: CATPPUCCIN.text,
        background: CATPPUCCIN.mauve,
        highlight: CATPPUCCIN.mauve,
        shadow: Color::Rgb(98, 83, 190),
    };
}
