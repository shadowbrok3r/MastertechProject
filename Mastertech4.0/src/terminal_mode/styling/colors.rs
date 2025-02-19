use crate::terminal_mode::widgets::button::Theme;
use ratatui::style::Color;

use super::CATPPUCCIN;

////////////////////////////////////
// Add color constants referencing the scheme
////////////////////////////////////
pub const C_DEEPPINK: Color = Color::Rgb(255, 20, 147);
pub const C_CYAN: Color = Color::Cyan;
pub const C_SPRINGGREEN: Color = Color::Rgb(0, 255, 127);
pub const C_MEDIUMSLATEBLUE: Color = Color::Rgb(123, 104, 238);
pub const C_DARKORANGE: Color = Color::Rgb(255, 140, 0);

// We'll define a single theme matching our desired turquoise color scheme.
// You can adjust highlight/shadow as you like.
pub const TURQUOISE: Theme = Theme {
    text: Color::Black,
    background: Color::Rgb(72, 209, 204), // mediumturquoise
    highlight: Color::Rgb(102, 239, 234), // lighten slightly for highlight
    shadow: Color::Rgb(42, 179, 174),     // darken slightly for shadow
};

pub const DEEPPINK: Theme = Theme {
    text: Color::Black,
    // Standard DeepPink in CSS is #FF1493 => (255, 20, 147)
    // If you want the exact standard color for all three, just do:
    background: Color::Rgb(255, 20, 147),
    highlight: Color::Rgb(255, 20, 147),
    shadow: Color::Rgb(255, 20, 147),
};

// Alternatively, if you prefer to use lighter/darker "shades" (from X11 DeepPink2, DeepPink3, etc.):
// const DEEPPINK: Theme = Theme {
//     text: Color::Black,
//     background: Color::Rgb(255, 20, 147), // DeepPink (#FF1493)
//     highlight: Color::Rgb(238, 18, 137),  // DeepPink2 (#EE1289)
//     shadow: Color::Rgb(205, 16, 118),     // DeepPink3 (#CD1076)
// };

pub const CYAN: Theme = Theme {
    text: Color::Black,
    // CSS “Cyan” is #00FFFF => (0, 255, 255)
    background: Color::Rgb(0, 255, 255),
    highlight: Color::Rgb(0, 255, 255),
    shadow: Color::Rgb(0, 255, 255),
};

pub const SPRINGGREEN: Theme = Theme {
    text: Color::Black,
    // CSS “SpringGreen” is #00FF7F => (0, 255, 127)
    background: Color::Rgb(0, 255, 127),
    highlight: Color::Rgb(0, 255, 127),
    shadow: Color::Rgb(0, 255, 127),
};

pub const MEDIUMSLATEBLUE: Theme = Theme {
    text: Color::Black,
    // CSS “MediumSlateBlue” is #7B68EE => (123, 104, 238)
    background: Color::Rgb(123, 104, 238),
    highlight: Color::Rgb(123, 104, 238),
    shadow: Color::Rgb(123, 104, 238),
};

pub const DARKORANGE: Theme = Theme {
    text: Color::Black,
    // CSS “DarkOrange” is #FF8C00 => (255, 140, 0)
    background: Color::Rgb(255, 140, 0),
    highlight: Color::Rgb(255, 140, 0),
    shadow: Color::Rgb(255, 140, 0),
};

pub const CATPPUCCINTHEME: Theme = Theme {
    text: CATPPUCCIN.pink,
    background: Color::Rgb(20, 20, 24),
    highlight: CATPPUCCIN.surface0,
    shadow: CATPPUCCIN.overlay1,
};