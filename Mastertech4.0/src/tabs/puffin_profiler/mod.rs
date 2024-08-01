// use eframe::egui::Ui;

// use crate::app_state::MastertechContext;

// impl MastertechContext {
//     pub fn puffin_profiler(&mut self, ui: &mut puffin_egui::egui::Ui){
//         puffin::profile_function!();
//         puffin::GlobalProfiler::lock().new_frame(); // call once per frame!
//         puffin_egui::profiler_ui(ui);
//     }
// }