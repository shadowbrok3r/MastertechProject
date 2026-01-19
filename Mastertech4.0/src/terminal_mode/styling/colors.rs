use crate::terminal_mode::widgets::button::Theme;
use ratatui::style::Color;
use super::{Catppuccin, CATPPUCCIN};
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

pub const DEEPPINK: Theme = Theme {
    // Text is now a darker tone of the vibrant deep pink.
    text: Color::Rgb(204, 16, 117), // 80% of (255, 20, 147)
    background: Color::Rgb(30, 5, 20),
    highlight: Color::Rgb(255, 20, 147),
    shadow: Color::Rgb(127, 10, 73),
};

pub const CYAN: Theme = Theme {
    // A darkened cyan provides a colorful yet subtle text option.
    text: Color::Rgb(0, 204, 204), // 80% of (0, 255, 255)
    background: Color::Rgb(10, 20, 20),
    highlight: Color::Rgb(0, 255, 255),
    shadow: Color::Rgb(0, 127, 127),
};

pub const SPRINGGREEN: Theme = Theme {
    // A slightly subdued spring green for the text.
    text: Color::Rgb(0, 204, 102), // 80% of (0, 255, 127)
    background: Color::Rgb(10, 20, 10),
    highlight: Color::Rgb(0, 255, 127),
    shadow: Color::Rgb(0, 127, 63),
};

pub const MEDIUMSLATEBLUE: Theme = Theme {
    // Darkened text preserves the cosmic slate vibe.
    text: Color::Rgb(98, 83, 190), // 80% of (123, 104, 238)
    background: Color::Rgb(10, 10, 20),
    highlight: Color::Rgb(123, 104, 238),
    shadow: Color::Rgb(61, 52, 119),
};

pub const DARKORANGE: Theme = Theme {
    // A rich, burnt orange text complements the vibrant highlight.
    text: Color::Rgb(204, 132, 0), // 80% of (255, 165, 0)
    background: Color::Rgb(30, 15, 5),
    highlight: Color::Rgb(255, 165, 0),
    shadow: Color::Rgb(127, 82, 0),
};



pub const CATPPUCCINTHEME: Theme = Theme {
    text: CATPPUCCIN.pink,
    background: Color::Rgb(20, 20, 24),
    highlight: CATPPUCCIN.surface0,
    shadow: CATPPUCCIN.overlay1,
};