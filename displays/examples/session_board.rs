//! Renders the session board on its own, without the rest of the app.

use displays::tabs::session_board::SessionBoard;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 950.0])
            .with_title("Session Board"),
        ..Default::default()
    };
    eframe::run_native(
        "session_board_example",
        opts,
        Box::new(|cc| {
            let mut fonts = eframe::egui::FontDefinitions::default();
            displays::ui_tools::icons::install_fonts(&mut fonts);
            cc.egui_ctx.set_fonts(fonts);
            cc.egui_ctx.set_visuals(eframe::egui::Visuals::dark());
            // Without the grab-pass backend every frost is a no-op and the nodes fall
            // back to their translucent fill.
            let glass = displays::ui_tools::glass_backdrop::install(cc);
            eprintln!("backdrop-blur available: {glass}");
            Ok(Box::new(Demo::default()))
        }),
    )
}

#[derive(Default)]
struct Demo {
    board: SessionBoard,
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.board.ui(ui);
    }
}
