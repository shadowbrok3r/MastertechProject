pub mod ticket_page;
pub mod software_page;
pub mod computer_page;
pub mod job_builder;
pub mod history_page;
pub mod diagnostics_page;

pub use ticket_page::display_ticket_page;
pub use software_page::display_software_page;
pub use computer_page::{display_computer_page, display_computer_page_with_search, ComputerSearchData};
pub use job_builder::display_job_builder_page;
pub use history_page::display_history_page;
pub use diagnostics_page::{display_diagnostics_page, DiagnosticSessionView};

pub fn return_colors(num: usize, _style: &eframe::egui::Style) -> Option<eframe::egui::Color32> {
    let mut _col = eframe::egui::Color32::from_rgb(30, 30, 38);
    if num % 2 == 0 {
        _col = eframe::egui::Color32::from_rgb(15, 15, 22);
    } else {
        _col = eframe::egui::Color32::from_rgb(30, 30, 38);
    }
    Some(_col)
}