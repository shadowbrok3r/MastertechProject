//! Chat markdown drawn without a row around it, using the shared chat renderer.

use eframe::egui::Ui;

use crate::ui_tools::chat_bubble::{self, ChatStyle};

/// Renders `text` as chat markdown into `ui`.
pub fn render(ui: &mut Ui, text: &str) {
    let style = ChatStyle::from_ui(ui);
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    chat_bubble::markdown(ui, &style, text, style.text, id);
}
