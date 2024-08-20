// pub mod part_order;

use std::fmt::Display;

// use eframe::egui::Ui;
// use egui_extras::Column;

pub trait DisplayableData {
    fn headers(&self) -> Vec<impl Display>; // Column headers for the table
    fn rows(&self) -> Vec<Vec<impl Display>>; // Data in row-major order
    // fn display_data<T: DisplayableData>(ui: &mut Ui, data: &[T]) {
    //     egui_extras::TableBuilder::new(ui)
    //         .striped(true)
    //         .resizable(true)
    //         .columns(
    //             Column::auto(), 
    //             data.len()
    //         )
    //         .header(height, add_header_row)
    //         .rows(data.iter().map(|item| {
    //             item.rows().into_iter().flatten().collect::<Vec<_>>()
    //         }))
    //         .show();
    // }
}

// struct ProcessInfo {
//     pid: u32,
//     name: String,
//     cpu_usage: f32,
// }

// impl DisplayableData for ProcessInfo {
//     fn headers(&self) -> Vec<impl Display> {
//         vec!["PID", "Name", "CPU Usage (%)"]
//     }

//     fn rows(&self) -> Vec<Vec<impl Display>> {
//         vec![vec![
//             self.pid.to_string(),
//             self.name.clone(),
//             format!("{:.2}", self.cpu_usage),
//         ]]
//     }
// }

// fn update_ui(ctx: &egui::CtxRef, processes: &[ProcessInfo], logs: &[EventLog], crashes: &[CrashReport]) {
//     egui::CentralPanel::default().show(ctx, |ui| {
//         ui.horizontal(|ui| {
//             ui.vertical(|ui| {
//                 ui.label("Processes");
//                 display_data(ui, processes);
//             });
//             ui.vertical(|ui| {
//                 ui.label("Event Logs");
//                 display_data(ui, logs);
//             });
//             ui.vertical(|ui| {
//                 ui.label("Crash Reports");
//                 display_data(ui, crashes);
//             });
//         });
//     });
// }