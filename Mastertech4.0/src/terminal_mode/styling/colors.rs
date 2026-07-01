use crate::terminal_mode::widgets::button::Theme;
use ratatui::style::Color;
use super::Catppuccin;
const COLORS: Catppuccin = Catppuccin::new();

/// The consistent app-wide background color (very dark, almost black)
/// Use this everywhere for background to ensure consistency across different terminals
pub const APP_BACKGROUND: Color = Color::Rgb(6, 6, 10);

pub static BASE_COLORS: [Color; 14] = [
    COLORS.rosewater,
    COLORS.flamingo,
    COLORS.pink,
    COLORS.mauve,
    COLORS.red,
    COLORS.maroon,
    COLORS.peach,
    COLORS.yellow,
    COLORS.green,
    COLORS.teal,
    COLORS.sky,
    COLORS.sapphire,
    COLORS.blue,
    COLORS.lavender,
];

pub const TURQUOISE: Theme = Theme {
    // Using an 80% brightness version of the highlight color.
    text: Color::Rgb(58, 167, 163), // derived from 80% of (72, 209, 204)
    background: Color::Rgb(15, 25, 35),
    highlight: Color::Rgb(72, 209, 204),
    shadow: Color::Rgb(36, 104, 102),
};

