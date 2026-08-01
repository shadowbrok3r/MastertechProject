//! Palette shared by every ratatui surface: the client's `terminal_mode` TUI and
//! the admin console's remote/beta terminals.

use std::ops::Deref;
use std::sync::atomic::{AtomicPtr, Ordering};

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

/// The consistent app-wide background color (very dark, almost black)
/// Use this everywhere for background to ensure consistency across different terminals
pub const APP_BACKGROUND: Color = Color::Rgb(6, 6, 10);

/// Four-color palette for a button or input widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub text: Color,
    pub background: Color,
    pub highlight: Color,
    pub shadow: Color,
}

pub const TURQUOISE: Theme = Theme {
    // Using an 80% brightness version of the highlight color.
    text: Color::Rgb(58, 167, 163), // derived from 80% of (72, 209, 204)
    background: Color::Rgb(15, 25, 35),
    highlight: Color::Rgb(72, 209, 204),
    shadow: Color::Rgb(36, 104, 102),
};

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

/// Terminal-mode palette resolved at render time from the active color scheme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppTheme {
    pub bg: Color,
    pub surface: Color,
    pub input_bg: Color,
    pub overlay: Color,
    pub border_muted: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub accent_shadow: Color,
    pub tertiary: Color,
    pub tertiary_shadow: Color,
    pub success: Color,
    pub error: Color,
    pub warning: Color,
}

/// Built-in default: pink accent + purple tertiary on the dark app background.
static DEFAULT_THEME: AppTheme = AppTheme {
    bg: APP_BACKGROUND,
    surface: CATPPUCCIN.surface0,
    input_bg: Color::Rgb(20, 20, 24),
    overlay: CATPPUCCIN.overlay1,
    border_muted: CATPPUCCIN.surface2,
    text: CATPPUCCIN.text,
    text_muted: CATPPUCCIN.subtext1,
    accent: Color::Rgb(255, 20, 147),
    accent_soft: CATPPUCCIN.pink,
    accent_shadow: Color::Rgb(127, 10, 73),
    tertiary: CATPPUCCIN.mauve,
    tertiary_shadow: Color::Rgb(98, 83, 190),
    success: CATPPUCCIN.green,
    error: CATPPUCCIN.red,
    warning: CATPPUCCIN.yellow,
};

/// Currently active theme; swapped atomically by `set_active_theme`.
static ACTIVE_THEME: AtomicPtr<AppTheme> =
    AtomicPtr::new((&DEFAULT_THEME as *const AppTheme).cast_mut());

/// Zero-sized handle dereferencing to the active `AppTheme`.
pub struct ActiveTheme;

impl Deref for ActiveTheme {
    type Target = AppTheme;
    fn deref(&self) -> &AppTheme {
        // Pointer is always DEFAULT_THEME or a leaked Box, both 'static.
        // Acquire pairs with the Release store so the pointee is fully visible.
        unsafe { &*ACTIVE_THEME.load(Ordering::Acquire) }
    }
}

pub static THEME: ActiveTheme = ActiveTheme;

/// Swaps the active theme. Leaks one `AppTheme` per call.
pub fn set_active_theme(theme: AppTheme) {
    ACTIVE_THEME.store(Box::leak(Box::new(theme)), Ordering::Release);
}

#[allow(dead_code)]
impl AppTheme {
    /// Border for focused / active panels.
    pub const fn border_active(&self) -> Color {
        self.accent
    }

    /// Muted border for idle panels.
    pub const fn border_idle(&self) -> Color {
        self.border_muted
    }

    /// Marker color for a checkbox/todo: accent when checked, tertiary when not.
    pub const fn checkbox(&self, checked: bool) -> Color {
        if checked { self.accent } else { self.tertiary }
    }

    /// Highlight style for the selected row in a menu / list.
    pub fn menu_highlight(&self) -> Style {
        Style::new().bg(self.surface).fg(self.accent).add_modifier(Modifier::BOLD)
    }

    /// Bold accent title style for panel headers.
    pub fn title(&self) -> Style {
        Style::new().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    /// Border style for a panel, accent when focused, muted otherwise.
    pub fn border(&self, focused: bool) -> Style {
        Style::new().fg(if focused { self.border_active() } else { self.border_idle() })
    }
}

/// Semantic button/input palette resolved against the active theme at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeRole {
    /// Primary-action buttons (Run, Submit).
    Accent,
    /// Muted secondary buttons.
    Neutral,
    /// Tertiary-control buttons.
    Tertiary,
    /// Text input fields.
    Input,
    /// Fixed palette independent of the active theme.
    Custom(Theme),
}

impl ThemeRole {
    pub fn resolve(self) -> Theme {
        let t = &*THEME;
        match self {
            ThemeRole::Accent => Theme {
                text: t.text,
                background: t.accent,
                highlight: t.accent,
                shadow: t.accent_shadow,
            },
            ThemeRole::Neutral => Theme {
                text: t.text_muted,
                background: t.border_muted,
                highlight: t.border_muted,
                shadow: t.surface,
            },
            ThemeRole::Tertiary => Theme {
                text: t.text,
                background: t.tertiary,
                highlight: t.tertiary,
                shadow: t.tertiary_shadow,
            },
            ThemeRole::Input => Theme {
                text: t.accent_soft,
                background: t.input_bg,
                highlight: t.surface,
                shadow: t.overlay,
            },
            ThemeRole::Custom(theme) => theme,
        }
    }
}

const fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

const fn color(rgb: [u8; 3]) -> Color {
    Color::Rgb(rgb[0], rgb[1], rgb[2])
}

/// Serializable color scheme persisted to `user.user_settings.tui_color_scheme`.
/// Missing fields deserialize from `Default` so old payloads survive new slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiColorScheme {
    pub name: String,
    pub bg: [u8; 3],
    pub surface: [u8; 3],
    pub input_bg: [u8; 3],
    pub overlay: [u8; 3],
    pub border_muted: [u8; 3],
    pub text: [u8; 3],
    pub text_muted: [u8; 3],
    pub accent: [u8; 3],
    pub accent_soft: [u8; 3],
    pub accent_shadow: [u8; 3],
    pub tertiary: [u8; 3],
    pub tertiary_shadow: [u8; 3],
    pub success: [u8; 3],
    pub error: [u8; 3],
    pub warning: [u8; 3],
}

impl Default for TuiColorScheme {
    fn default() -> Self {
        Self::deep_pink()
    }
}

impl TuiColorScheme {
    pub fn to_app_theme(&self) -> AppTheme {
        AppTheme {
            bg: color(self.bg),
            surface: color(self.surface),
            input_bg: color(self.input_bg),
            overlay: color(self.overlay),
            border_muted: color(self.border_muted),
            text: color(self.text),
            text_muted: color(self.text_muted),
            accent: color(self.accent),
            accent_soft: color(self.accent_soft),
            accent_shadow: color(self.accent_shadow),
            tertiary: color(self.tertiary),
            tertiary_shadow: color(self.tertiary_shadow),
            success: color(self.success),
            error: color(self.error),
            warning: color(self.warning),
        }
    }

    /// Installs this scheme as the active theme.
    pub fn apply(&self) {
        set_active_theme(self.to_app_theme());
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        serde_json::from_slice(bytes).ok()
    }

    /// Editable slots as (label, value) pairs, ordered for the settings UI.
    pub fn slots(&self) -> [(&'static str, [u8; 3]); 15] {
        [
            ("Background", self.bg),
            ("Surface", self.surface),
            ("Input Background", self.input_bg),
            ("Overlay", self.overlay),
            ("Muted Border", self.border_muted),
            ("Text", self.text),
            ("Muted Text", self.text_muted),
            ("Accent", self.accent),
            ("Accent Soft", self.accent_soft),
            ("Accent Shadow", self.accent_shadow),
            ("Tertiary", self.tertiary),
            ("Tertiary Shadow", self.tertiary_shadow),
            ("Success", self.success),
            ("Error", self.error),
            ("Warning", self.warning),
        ]
    }

    /// Writes a slot by its `slots()` index.
    pub fn set_slot(&mut self, index: usize, value: [u8; 3]) {
        let slot = match index {
            0 => &mut self.bg,
            1 => &mut self.surface,
            2 => &mut self.input_bg,
            3 => &mut self.overlay,
            4 => &mut self.border_muted,
            5 => &mut self.text,
            6 => &mut self.text_muted,
            7 => &mut self.accent,
            8 => &mut self.accent_soft,
            9 => &mut self.accent_shadow,
            10 => &mut self.tertiary,
            11 => &mut self.tertiary_shadow,
            12 => &mut self.success,
            13 => &mut self.error,
            14 => &mut self.warning,
            _ => return,
        };
        *slot = value;
    }

    pub fn presets() -> Vec<TuiColorScheme> {
        vec![
            Self::deep_pink(),
            Self::amoled_crimson(),
            Self::catppuccin_mocha(),
            Self::turquoise(),
            Self::cyan(),
            Self::spring_green(),
            Self::slate_blue(),
            Self::dark_orange(),
        ]
    }

    pub fn deep_pink() -> Self {
        Self {
            name: "Deep Pink".into(),
            bg: [6, 6, 10],
            surface: hex(0x313244),
            input_bg: [20, 20, 24],
            overlay: hex(0x7f849c),
            border_muted: hex(0x585b70),
            text: hex(0xcdd6f4),
            text_muted: hex(0xbac2de),
            accent: [255, 20, 147],
            accent_soft: hex(0xf5c2e7),
            accent_shadow: [127, 10, 73],
            tertiary: hex(0xcba6f7),
            tertiary_shadow: [98, 83, 190],
            success: hex(0xa6e3a1),
            error: hex(0xf38ba8),
            warning: hex(0xf9e2af),
        }
    }

    pub fn amoled_crimson() -> Self {
        Self {
            name: "AMOLED Crimson".into(),
            bg: [0, 0, 0],
            surface: [26, 10, 16],
            input_bg: [12, 4, 8],
            overlay: [140, 100, 115],
            border_muted: [64, 28, 42],
            text: [235, 230, 235],
            text_muted: [160, 150, 158],
            accent: [255, 45, 85],
            accent_soft: [255, 120, 150],
            accent_shadow: [128, 22, 42],
            tertiary: [255, 20, 147],
            tertiary_shadow: [127, 10, 73],
            success: hex(0xa6e3a1),
            error: hex(0xf38ba8),
            warning: hex(0xf9e2af),
        }
    }

    pub fn catppuccin_mocha() -> Self {
        Self {
            name: "Catppuccin Mocha".into(),
            bg: hex(0x1e1e2e),
            surface: hex(0x313244),
            input_bg: hex(0x181825),
            overlay: hex(0x7f849c),
            border_muted: hex(0x585b70),
            text: hex(0xcdd6f4),
            text_muted: hex(0xbac2de),
            accent: hex(0xf5c2e7),
            accent_soft: hex(0xf2cdcd),
            accent_shadow: [122, 97, 115],
            tertiary: hex(0xcba6f7),
            tertiary_shadow: [101, 83, 123],
            success: hex(0xa6e3a1),
            error: hex(0xf38ba8),
            warning: hex(0xf9e2af),
        }
    }

    pub fn turquoise() -> Self {
        Self {
            name: "Turquoise".into(),
            bg: [5, 9, 12],
            surface: [21, 33, 43],
            input_bg: [13, 21, 28],
            overlay: hex(0x7f849c),
            border_muted: [50, 72, 88],
            text: hex(0xcdd6f4),
            text_muted: hex(0xbac2de),
            accent: [72, 209, 204],
            accent_soft: hex(0x94e2d5),
            accent_shadow: [36, 104, 102],
            tertiary: hex(0x74c7ec),
            tertiary_shadow: [58, 99, 118],
            success: hex(0xa6e3a1),
            error: hex(0xf38ba8),
            warning: hex(0xf9e2af),
        }
    }

    pub fn cyan() -> Self {
        Self {
            name: "Cyan".into(),
            bg: [4, 9, 9],
            surface: [16, 34, 34],
            input_bg: [10, 20, 20],
            overlay: hex(0x7f849c),
            border_muted: [42, 76, 76],
            text: hex(0xcdd6f4),
            text_muted: hex(0xbac2de),
            accent: [0, 255, 255],
            accent_soft: hex(0x89dceb),
            accent_shadow: [0, 127, 127],
            tertiary: hex(0x94e2d5),
            tertiary_shadow: [74, 113, 106],
            success: hex(0xa6e3a1),
            error: hex(0xf38ba8),
            warning: hex(0xf9e2af),
        }
    }

    pub fn spring_green() -> Self {
        Self {
            name: "Spring Green".into(),
            bg: [4, 10, 6],
            surface: [17, 36, 25],
            input_bg: [10, 20, 13],
            overlay: hex(0x7f849c),
            border_muted: [40, 80, 56],
            text: hex(0xcdd6f4),
            text_muted: hex(0xbac2de),
            accent: [0, 255, 127],
            accent_soft: hex(0xa6e3a1),
            accent_shadow: [0, 127, 63],
            tertiary: hex(0x94e2d5),
            tertiary_shadow: [74, 113, 106],
            success: hex(0xa6e3a1),
            error: hex(0xf38ba8),
            warning: hex(0xf9e2af),
        }
    }

    pub fn slate_blue() -> Self {
        Self {
            name: "Slate Blue".into(),
            bg: [6, 6, 14],
            surface: [26, 26, 46],
            input_bg: [14, 14, 26],
            overlay: hex(0x7f849c),
            border_muted: [56, 56, 92],
            text: hex(0xcdd6f4),
            text_muted: hex(0xbac2de),
            accent: [123, 104, 238],
            accent_soft: hex(0xb4befe),
            accent_shadow: [61, 52, 119],
            tertiary: hex(0x89b4fa),
            tertiary_shadow: [68, 90, 125],
            success: hex(0xa6e3a1),
            error: hex(0xf38ba8),
            warning: hex(0xf9e2af),
        }
    }

    pub fn dark_orange() -> Self {
        Self {
            name: "Dark Orange".into(),
            bg: [11, 8, 4],
            surface: [40, 30, 18],
            input_bg: [22, 16, 9],
            overlay: hex(0x7f849c),
            border_muted: [88, 66, 40],
            text: hex(0xcdd6f4),
            text_muted: hex(0xbac2de),
            accent: [255, 165, 0],
            accent_soft: hex(0xfab387),
            accent_shadow: [127, 82, 0],
            tertiary: hex(0xeba0ac),
            tertiary_shadow: [117, 80, 86],
            success: hex(0xa6e3a1),
            error: hex(0xf38ba8),
            warning: hex(0xf9e2af),
        }
    }
}
