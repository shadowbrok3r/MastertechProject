use std::sync::{Arc, Mutex};
use anyhow::Context;
use ratatui::{prelude::*, widgets::{Block, Borders}};
use file_explorer::{File, FileExplorer, Theme};
use crate::terminal_mode::{context::TerminalContext, styling::THEME};

pub mod render;
pub mod file_explorer;

/// A tab that mimics *ncdu* by embedding `ratatui-explorer`.
pub struct NcduTab {
    _ctx: Arc<Mutex<TerminalContext>>, // allows cross‑tab shared state
    explorer: FileExplorer,           // the main widget
    layout: Layout,                   // cached layout description
    // sizes: SizeCache,
}

impl NcduTab {
    /// Create a new `NcduTab` rooted at `initial_path` (or current dir).
    pub fn new(_ctx: Arc<Mutex<TerminalContext>>) -> Self {
        // Build a theme consistent with your global style guide
        let theme = Theme::default()
            // .with_highlight_symbol("=> ")
            .add_default_title()
            .with_block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Rounded).border_style(Style::default().fg(THEME.accent)))
            .with_dir_style(Style::default().fg(THEME.tertiary).add_modifier(Modifier::BOLD))
            .with_highlight_item_style(
                Style::default()
                    .fg(THEME.accent)
                    .add_modifier(Modifier::BOLD)
                    .bg(THEME.surface),
            )
            .with_highlight_dir_style(
                Style::default()
                    .fg(THEME.accent)
                    .add_modifier(Modifier::BOLD)
                    .bg(THEME.surface),
            )
            .with_scroll_padding(1);

        // Instantiate the explorer and optionally jump to a custom root.
        let explorer = FileExplorer::with_theme(theme).unwrap();

        // 1/3 – 2/3 horizontal split is typical ncdu‑like layout.
        let layout = Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)]);

        Self { _ctx, explorer, layout } // , sizes: SizeCache::default()
    }

    pub fn _receive(&mut self) {

    }
}

fn get_file_content(file: &File) -> anyhow::Result<std::borrow::Cow<'_, str>, anyhow::Error> {
    if file.is_file() {
        std::fs::read_to_string(file.path())
            .map(std::borrow::Cow::from)
            .with_context(|| format!("reading {}", file.path().display()))
    } else if file.is_dir() {
        Ok(std::borrow::Cow::Borrowed(""))
    } else {
        Ok(std::borrow::Cow::Borrowed("<not a regular file>"))
    }
}
