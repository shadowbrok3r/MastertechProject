pub mod ticket_page;
pub mod software_page;
pub mod computer_page;
pub mod job_builder;
pub mod history_page;
pub mod diagnostics_page;
pub mod stress_tests_page;
pub mod ai_checklist_panel;

pub use ticket_page::display_ticket_page;
pub use software_page::display_software_page;
pub use computer_page::{display_computer_page, display_computer_page_with_search, ComputerSearchData};
pub use job_builder::display_job_builder_page;
pub use history_page::display_history_page;
pub use diagnostics_page::{display_diagnostics_page, DiagnosticSessionView};
pub use stress_tests_page::display_stress_tests_page;
pub use ai_checklist_panel::{ai_checklist_progress, display_ai_checklist};

use eframe::egui::{AsIdSalt, ScrollArea, Ui, UiBuilder};

/// Shows `scroll` over `ui` widened by `bleed`; content is `ui`'s width less the bar width `bleed` leaves uncovered.
pub(crate) fn bar_scroll<R>(ui: &mut Ui, scroll: ScrollArea, bleed: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let bar = ui.spacing().scroll.allocated_width();
    let bleed = bleed.clamp(0.0, bar);
    let content_w = ui.available_width() - (bar - bleed);
    let mut rect = ui.available_rect_before_wrap();
    rect.max.x += bleed;
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        scroll
            .show(ui, |ui| {
                let mut content = ui.max_rect();
                content.set_width(content_w);
                ui.scope_builder(UiBuilder::new().max_rect(content), add).inner
            })
            .inner
    })
    .inner
}

/// A page-filling [`bar_scroll`] with no bleed.
pub(crate) fn page_scroll<R>(ui: &mut Ui, id_salt: impl AsIdSalt, add: impl FnOnce(&mut Ui) -> R) -> R {
    bar_scroll(ui, ScrollArea::vertical().id_salt(id_salt).auto_shrink(false), 0.0, add)
}

pub fn return_colors(num: usize, _style: &eframe::egui::Style) -> Option<eframe::egui::Color32> {
    let mut _col = eframe::egui::Color32::from_rgb(30, 30, 38);
    if num % 2 == 0 {
        _col = eframe::egui::Color32::from_rgb(15, 15, 22);
    } else {
        _col = eframe::egui::Color32::from_rgb(30, 30, 38);
    }
    Some(_col)
}