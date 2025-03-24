#![allow(dead_code)]
pub mod tokyo_dark;
pub mod github;

use super::syntax::TokenType;
use eframe::egui::Color32;

pub const ERROR_COLOR: Color32 = Color32::from_rgb(255, 0, 255);

/// Array of default themes.
pub const DEFAULT_THEMES: [ColorTheme; 2] = [
    ColorTheme::TOKYO_DARK,
    ColorTheme::GITHUB_DARK,
];


fn color_from_hex(hex: &str) -> Option<Color32> {
    if hex == "none" {
        return Some(Color32::from_rgba_premultiplied(255, 0, 255, 0));
    }
    let rgb = (1..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect::<Vec<u8>>();
    let color = Color32::from_rgb(*rgb.first()?, *rgb.get(1)?, *rgb.get(2)?);
    Some(color)
}

#[derive(Hash, Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// Colors in hexadecimal notation as used in HTML and CSS.
pub struct ColorTheme {
    pub name: &'static str,
    pub dark: bool,
    pub bg: &'static str,
    pub cursor: &'static str,
    pub selection: &'static str,
    pub comments: &'static str,
    pub functions: &'static str,
    pub keywords: &'static str,
    pub literals: &'static str,
    pub numerics: &'static str,
    pub punctuation: &'static str,
    pub strs: &'static str,
    pub types: &'static str,
    pub special: &'static str,
    pub variable: &'static str,
    pub symbol: &'static str,
    pub embedded: &'static str,
    pub subexpression: &'static str,
    pub embeddedvariable: &'static str,
    pub afterdollarinstring: &'static str,
}

impl Default for ColorTheme {
    fn default() -> Self {
        ColorTheme::TOKYO_DARK
    }
}

impl ColorTheme {
    pub fn name(&self) -> &str {
        self.name
    }

    pub fn is_dark(&self) -> bool {
        self.dark
    }
    
    pub fn bg(&self) -> Color32 {
        color_from_hex(self.bg).unwrap_or(ERROR_COLOR)
    }
    
    pub fn cursor(&self) -> Color32 {
        color_from_hex(self.cursor).unwrap_or(ERROR_COLOR)
    }
    
    pub fn selection(&self) -> Color32 {
        color_from_hex(self.selection).unwrap_or(ERROR_COLOR)
    }
    
    pub fn modify_style(&self, ui: &mut eframe::egui::Ui, fontsize: f32) {
        let style = ui.style_mut();
        style.visuals.widgets.noninteractive.bg_fill = self.bg();
        style.visuals.window_fill = self.bg();
        style.visuals.selection.stroke.color = self.cursor();
        style.visuals.selection.bg_fill = self.selection();
        style.visuals.extreme_bg_color = self.bg();
        style.override_font_id = Some(eframe::egui::FontId::monospace(fontsize));
        style.visuals.text_cursor.stroke.width = fontsize * 0.1;
    }

    pub const fn type_color_str(&self, ty: TokenType) -> &'static str {
        match ty {
            TokenType::Comment(_) => self.comments,
            TokenType::Function => self.functions,
            TokenType::Keyword => self.keywords,
            TokenType::Literal => self.literals,
            TokenType::Hyperlink => self.special,
            TokenType::Numeric(_) => self.numerics,
            TokenType::Punctuation(_) => self.punctuation,
            TokenType::Special => self.special,
            TokenType::Str(_) => self.strs,
            TokenType::Embedded(_) => self.embedded,
            TokenType::Type => self.types,
            TokenType::Variable => self.variable,
            TokenType::Symbol => self.symbol,
            TokenType::Whitespace(_) | TokenType::Unknown => self.comments,
            TokenType::Subexpression(_) => self.subexpression,
            TokenType::EmbeddedVariable(_) => self.embeddedvariable,
            TokenType::AfterDollarInString(_) => self.afterdollarinstring,
        }
    }
    
    
    
    pub fn type_color(&self, ty: TokenType) -> Color32 {
        match ty {
            TokenType::Comment(_) => color_from_hex(self.comments),
            TokenType::Function => color_from_hex(self.functions),
            TokenType::Keyword => color_from_hex(self.keywords),
            TokenType::Literal => color_from_hex(self.literals),
            TokenType::Hyperlink => color_from_hex(self.special),
            TokenType::Numeric(_) => color_from_hex(self.numerics),
            TokenType::Punctuation(_) => color_from_hex(self.punctuation),
            TokenType::Special => color_from_hex(self.special),
            TokenType::Str(_) => color_from_hex(self.strs),
            TokenType::Type => color_from_hex(self.types),
            TokenType::Variable => color_from_hex(self.variable),
            TokenType::Whitespace(_) | TokenType::Unknown => color_from_hex(self.comments),
            TokenType::Symbol => color_from_hex(self.symbol),
            TokenType::Embedded(_) => color_from_hex(self.embedded),
            TokenType::Subexpression(_) => color_from_hex(self.subexpression),
            TokenType::EmbeddedVariable(_) => color_from_hex(self.embeddedvariable),
            TokenType::AfterDollarInString(_) => color_from_hex(self.afterdollarinstring),
        }
        .unwrap_or(ERROR_COLOR)
    }
    

    pub fn monocolor(
        dark: bool,
        bg: &'static str,
        fg: &'static str,
        cursor: &'static str,
        selection: &'static str,
    ) -> Self {
        ColorTheme {
            name: "monocolor",
            dark,
            bg,
            cursor,
            selection,
            literals: fg,
            numerics: fg,
            keywords: fg,
            functions: fg,
            punctuation: fg,
            types: fg,
            strs: fg,
            comments: fg,
            special: fg,
            variable: fg,
            symbol: fg,
            embedded: fg,
            subexpression: fg,
            embeddedvariable: fg,
            afterdollarinstring: fg,
        }
    }

    pub fn color_for_scope(&self, scope: &str) -> Option<Color32> {
        // Match the most specific scope first
        if scope.contains("keyword.control.powershell") {
            Color32::from_hex(self.keywords).ok()
        } else if scope.contains("comment") {
            Color32::from_hex(self.comments).ok()
        } else if scope.contains("entity.name.function.powershell") {
            Color32::from_hex(self.functions).ok()
        } else if scope.contains("variable.other.powershell") {
            Color32::from_hex(self.variable).ok()
        } else if scope.contains("variable.parameter.powershell") {
            Color32::from_hex(self.variable).ok()
        } else if scope.contains("constant.numeric.powershell") {
            Color32::from_hex(self.numerics).ok()
        } else if scope.contains("string.quoted") {
            Color32::from_hex(self.strs).ok()
        } else if scope.contains("punctuation") {
            Color32::from_hex(self.punctuation).ok()
        } else if scope.contains("source.powershell.embedded") {
            Color32::from_hex(self.subexpression).ok()
        } else {
            None
        }
    }

    pub fn fg(&self) -> Color32 {
        Color32::from_hex(self.literals).unwrap_or(Color32::WHITE)
    }
    
    pub fn bg_for_scope(&self, _scope: &str) -> Option<Color32> {
        None // Add background colors per scope if needed
    }
}