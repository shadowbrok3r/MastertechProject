use eframe::egui;
use egui::{Stroke, style::Spacing, Margin};
use egui_extras::{Size, StripBuilder};

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
        let mut style = (cc.egui_ctx.style()).clone(); //let mut style: egui::Style = (*ctx.style()).clone();
        //cc.egui_ctx.set_style(egui::Style::)
        //style.spacing.item_spacing = egui::vec2(10.0, 20.0);
        //ctx.set_style(style);
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
        let mut style = (*ctx.style()).clone();

        style.spacing.window_margin = Margin {
            left: 5.,
            right: 5.,
            top: 300.,
            bottom: 5.,
        };

        ctx.set_style(style);
        egui::CentralPanel::default().show(ctx, |ui| {

                ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(144, 238, 144));
                ui.horizontal(|ui| {

                    // Add tabs
                    if ui.selectable_label(self.active_tab == 0, "Main").clicked() {
                        self.active_tab = 0;
                    }else if ui.selectable_label(self.active_tab == 1, "Scripts").clicked() {
                        self.active_tab = 1;
                    }
                
            });


            if self.active_tab == 0 {
                StripBuilder::new(ui)
                    .size(Size::relative(0.5))// top cell
                    .size(Size::relative(0.25))
                    .vertical(|mut strip| {
                        strip.strip(|builder| {
                            builder.sizes(Size::remainder(), 2).horizontal(|mut strip| {
                                strip.cell(|ui| {
                                    ui.painter().rect_filled(
                                        ui.available_rect_before_wrap(),
                                        0.0,
                                        egui::Color32::GREEN,
                                    );
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut self.so_number).hint_text("SO#").char_limit(8)
                                        .vertical_align(egui::Align::Center)
                                        .desired_width(130.0));
        
                                        ui.add(egui::TextEdit::singleline(&mut self.customer_name).hint_text("Customer Name")
                                        .desired_width(130.0));
                                    });
        
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut self.phone1).hint_text("Phone Number 1")
                                        .desired_width(130.0));
                                    
                                        ui.add(egui::TextEdit::singleline(&mut self.phone2).hint_text("Phone Number 2")
                                        .desired_width(130.0));      
                                    });
                                });
                                strip.strip(|builder| {
                                    builder.sizes(Size::remainder(), 1).horizontal(|mut strip| {
                                        strip.cell(|ui| {
                                            ui.painter().rect_filled(
                                                ui.available_rect_before_wrap(),
                                                0.0,
                                                egui::Color32::BLUE,
                                            );
                                            ui.label("width: 50%\nheight: 1/3 of the red region");
                                        });
                                    });
                                });
                            });
                        });
                    });

            }if self.active_tab == 1 {
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