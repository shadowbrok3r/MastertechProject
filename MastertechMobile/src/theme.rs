use dioxus::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub bg: String,
    pub panel: String,
    pub border: String,
    pub accent: String,
    pub text: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            bg: "#0b0b0f".into(),
            panel: "#0c0c10".into(),
            border: "#2a2c5d".into(),
            accent: "#0bf4c0".into(),
            text: "#f1f5f9".into(),
        }
    }
}

impl ThemeConfig {
    pub fn to_css_vars(&self) -> String {
        format!(":root{{--mt-bg:{};--mt-panel:{};--mt-border:{};--mt-accent:{};--mt-text:{};}}", self.bg, self.panel, self.border, self.accent, self.text)
    }
}

#[component]
#[allow(unused_braces)]
pub fn ThemeStyle(css: String) -> Element { rsx!( style { {css} } ) }

pub fn apply_theme_signal(theme_sig: &mut Signal<ThemeConfig>, new_theme: ThemeConfig) {
    theme_sig.set(new_theme);
}
