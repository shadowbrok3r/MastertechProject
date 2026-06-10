//! Terminal-mode theme ported to the UEFI app: Catppuccin palette plus the
//! pink-accent / purple-tertiary `AppTheme` used by Mastertech terminal mode.
//!
//! Colors are the true RGB values from `Mastertech4.0/src/terminal_mode/styling`;
//! the ratatui-uefi backend quantizes them to the nearest EFI text color.

use ratatui::style::{Color, Modifier, Style};

/// The consistent app-wide background color (very dark, almost black).
pub const APP_BACKGROUND: Color = Color::Rgb(6, 6, 10);

pub const CATPPUCCIN: Catppuccin = Catppuccin::new();

#[allow(unused)]
pub struct Catppuccin {
    pub rosewater: Color,
    pub flamingo: Color,
    pub pink: Color,
    pub mauve: Color,
    pub red: Color,
    pub maroon: Color,
    pub peach: Color,
    pub yellow: Color,
    pub green: Color,
    pub teal: Color,
    pub sky: Color,
    pub sapphire: Color,
    pub blue: Color,
    pub lavender: Color,
    pub text: Color,
    pub subtext1: Color,
    pub subtext0: Color,
    pub overlay2: Color,
    pub overlay1: Color,
    pub overlay0: Color,
    pub surface2: Color,
    pub surface1: Color,
    pub surface0: Color,
    pub base: Color,
    pub mantle: Color,
    pub crust: Color,
}

impl Catppuccin {
    pub const fn new() -> Self {
        Self {
            rosewater: Color::from_u32(0xf5e0dc),
            flamingo: Color::from_u32(0xf2cdcd),
            pink: Color::from_u32(0xf5c2e7),
            mauve: Color::from_u32(0xcba6f7),
            red: Color::from_u32(0xf38ba8),
            maroon: Color::from_u32(0xeba0ac),
            peach: Color::from_u32(0xfab387),
            yellow: Color::from_u32(0xf9e2af),
            green: Color::from_u32(0xa6e3a1),
            teal: Color::from_u32(0x94e2d5),
            sky: Color::from_u32(0x89dceb),
            sapphire: Color::from_u32(0x74c7ec),
            blue: Color::from_u32(0x89b4fa),
            lavender: Color::from_u32(0xb4befe),
            text: Color::from_u32(0xcdd6f4),
            subtext1: Color::from_u32(0xbac2de),
            subtext0: Color::from_u32(0xa6adc8),
            overlay2: Color::from_u32(0x9399b2),
            overlay1: Color::from_u32(0x7f849c),
            overlay0: Color::from_u32(0x6c7086),
            surface2: Color::from_u32(0x585b70),
            surface1: Color::from_u32(0x45475a),
            surface0: Color::from_u32(0x313244),
            base: Color::from_u32(0x1e1e2e),
            mantle: Color::from_u32(0x181825),
            crust: Color::from_u32(0x11111b),
        }
    }
}

/// Unified palette: pink accent + purple tertiary on the dark app background.
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

    /// Bold pink title style for panel headers.
    pub fn title(&self) -> Style {
        Style::new().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    /// Border style for a panel, pink when focused, muted otherwise.
    pub fn border(&self, focused: bool) -> Style {
        Style::new().fg(if focused { self.border_active() } else { self.border_idle() })
    }
}
