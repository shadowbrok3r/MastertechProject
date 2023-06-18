use eframe::egui;
//use egui::Visuals;

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("Mastertech", native_options, Box::new(|_| Box::new(MastertechApp::default())));
}

#[derive(Default)]
struct MastertechApp {
    so_number: String,
    customer_name: String,
    phone1: String,
    phone2: String,
    active_tab: usize,
}

impl MastertechApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Get current context style
        let style = (cc.egui_ctx.style()).clone();
        //let colors = egui::Color32::from_rgb(0,0,0);
        
        
        //ctx.set_visuals(egui::Visuals::dark()); 
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        Self::default()
    }
}

impl eframe::App for MastertechApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(144, 238, 144));
            ui.horizontal(|ui| {
                // Add tabs
                if ui.selectable_label(self.active_tab == 0, "Main").clicked() {
                    self.active_tab = 0;
                }else if ui.selectable_label(self.active_tab == 1, "Scripts").clicked() {
                    self.active_tab = 1;
                }
                // Add more tabs as needed
            });

            if self.active_tab == 0 {
                // Add text fields
                ui.with_layout(Layout::left_to_right, Align::left_top)(|ui| {
                    ui.columns(2, |cols| {
                        cols[0].add(egui::TextEdit::singleline(&mut self.so_number).hint_text("SO#"));
                        cols[1].add(egui::TextEdit::singleline(&mut self.customer_name).hint_text("Customer Name"));
                    });
                    ui.columns(2, |cols| {
                        cols[0].add(egui::TextEdit::singleline(&mut self.phone1).hint_text("Phone Number 1"));
                        cols[1].add(egui::TextEdit::singleline(&mut self.phone2).hint_text("Phone Number 2"));
                    });
                });
            }else if self.active_tab == 1 {
                // Add text fields
                ui.vertical_centered(|ui| {
                    ui.columns(2, |cols| {
                        cols[0].add(egui::TextEdit::singleline(&mut self.so_number).hint_text("SO#"));
                        cols[1].add(egui::TextEdit::singleline(&mut self.customer_name).hint_text("Customer Name"));
                    });
                    ui.columns(2, |cols| {
                        cols[0].add(egui::TextEdit::singleline(&mut self.phone1).hint_text("Phone Number 1"));
                        cols[1].add(egui::TextEdit::singleline(&mut self.phone2).hint_text("Phone Number 2"));
                    });
                });
            }
            // Add more tab contents as needed
        });
    }
}
