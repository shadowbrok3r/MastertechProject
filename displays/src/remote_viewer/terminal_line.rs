use eframe::egui::{*, text::LayoutJob};

#[derive(Default)]
pub struct TerminalLine(pub LayoutJob);

impl Widget for TerminalLine {
    fn ui(self, ui: &mut Ui) -> Response {
        let galley = ui.fonts_mut(|f| f.layout_job(self.0)); // Layout the text
        let size = galley.rect.size();
        let (response, painter) = ui.allocate_painter(size, eframe::egui::Sense::click()); // Allocate space
        painter.galley(response.rect.min, galley, Color32::WHITE); // Paint with fallback color
        response
    }
}


// #[must_use = "You should put this widget in an ui with `ui.add(widget);`"]
// pub struct TerminalLine {
//     job: LayoutJob,
// }

// impl TerminalLine {
//     pub fn new(job: LayoutJob) -> Self {
//         Self { job }
//     }

//     pub fn job(self) -> LayoutJob {
//         self.job
//     }
// }

// impl Widget for TerminalLine {
//     fn ui(self, ui: &mut Ui) -> Response {
//         let galley = ui.fonts(|fonts| fonts.layout_job(self.job()));

//         /* let boop = ui.allocate_ui(galley.size(), |ui| {  ui.painter().add(
//                    epaint::TextShape::new(galley.rect.left_top(), galley.clone(), ui.style().visuals.text_color())

//                );});
//         */

//         // let bigger = galley.size() + vec2(300.0, -1.0);

//         let (response, painter) = ui.allocate_painter(galley.size(), Sense::hover());

//         painter.galley(
//             response.rect.left_top(),
//             galley.clone(),
//             ui.style().visuals.text_color(),
//         );

//         response
//     }
// }