use eframe::egui;
use egui::{*, epaint::Shadow};
use egui_extras::{Size, StripBuilder};

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("Mastertech", native_options, Box::new(|_| Box::new(MastertechApp::default())));
}
#[derive(Debug, PartialEq)]
enum Salesman {
    Jake,
    Danny
}
#[derive(Debug, PartialEq)]
enum Techs{
    Logan,
    Bread,
    Taco
}

pub struct MastertechApp {
    //////////////////////////////////////////
    /*          Mastertech Vars             */
    //////////////////////////////////////////
    so_number: String,
    customer_name: String,
    phone1: String,
    phone2: String,
    salesman_cbox: Salesman,
    techs_cbox: Techs,
    checkin_notes: String,

    //////////////////////////////////////////
    /*          Widgets and UI elements     */
    //////////////////////////////////////////
    widget_size: f32,
    active_tab: usize,
    default_margins: Margin,
    default_frame: Frame,

    //////////////////////////////////////////
    /*          UI Colors                   */
    //////////////////////////////////////////
    purple_color: Color32,
    purple_stroke: Stroke,
    strip_bg_color: Color32,
    text_color: Color32,
    window_fill_color: Color32,
}

impl Default for MastertechApp {
    fn default() -> Self {
        Self {
            //////////////////////////////////////////
            /*          Mastertech Vars             */
            //////////////////////////////////////////
            so_number: "".to_string(),
            customer_name: "".to_string(),
            phone1: "".to_string(),
            phone2: "".to_string(),
            salesman_cbox: Salesman::Jake,
            techs_cbox: Techs::Logan,
            checkin_notes: "".to_string(),

            //////////////////////////////////////////
            /*          Widgets and UI elements     */
            //////////////////////////////////////////
            widget_size: 130.0,
            active_tab: 0,
            default_margins: Margin::same(10.0),
            default_frame: Frame{
                inner_margin: Margin::same(10.0), outer_margin: Margin::same(10.0),
                rounding: Rounding::same(10.0), shadow: Shadow::big_light(),
                fill: Color32::BLACK, stroke: Stroke { width: 1.0, color: Color32::LIGHT_GREEN },
            },

            //////////////////////////////////////////
            /*          UI Colors                   */
            //////////////////////////////////////////
            strip_bg_color: Color32::from_rgb(43, 41, 51),
            purple_color: Color32::from_rgb_additive(145, 29, 122),
            text_color: Color32::from_rgb(24, 186, 135),
            window_fill_color: Color32::from_rgb(38, 44, 56),
            purple_stroke: Stroke::new(1.0, Color32::from_rgb_additive(145, 29, 122))
        }
    }
}

impl MastertechApp {
    
}


impl eframe::App for MastertechApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        
        let mut style = (*ctx.style()).clone();
        style.spacing.combo_width = self.widget_size;
        //style.spacing.button_padding = vec2(8.0, 8.0); //This is causing weirdness with only the Tech comboBox...
        style.visuals.window_stroke = Stroke{width:1.0, color: self.purple_color};
        style.visuals.dark_mode = true;
        style.visuals.window_fill = self.window_fill_color;
        style.spacing.item_spacing = vec2(8.0, 8.0);

        ctx.set_style(style);


        SidePanel::new(panel::Side::Right, "console_window")
        .resizable(true)
        .width_range(std::ops::RangeInclusive::new(150.0, 400.0))
        .show(ctx, |ui| {
            
            ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.strip_bg_color);
                ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.purple_stroke);
                ui.horizontal(|ui|{

                });
            });
        });

        CentralPanel::default()
            .frame(Frame::inner_margin(self.default_frame, self.default_margins))
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.visuals_mut().override_text_color = Some(self.text_color);
                // For the top level tabs
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
                    .size(Size::relative(0.5))// top strip
                    .size(Size::relative(0.5)) // bottom strip
                    .horizontal(|mut strip| {
                        strip.strip(|builder| {
                            builder.sizes(Size::remainder(), 1).horizontal(|mut strip| {
                                strip.cell(|ui|{
                                    ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.strip_bg_color);
                                    ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.purple_stroke);
                                    

                                    ui.vertical(|ui| {ui.add_space(3.0);}); // leave some margin above the textEdits

                                    
                                    ui.horizontal(|ui|{
                                        ui.add_space(15.0);
                                        ui.add(TextEdit::singleline(&mut self.so_number)
                                        .hint_text("SO#").char_limit(8).desired_width(self.widget_size));
                                        
                                        ui.add(TextEdit::singleline(&mut self.customer_name)
                                        .hint_text("Customer Name").desired_width(self.widget_size));
                                    });
        
                                    ui.horizontal(|ui| {
                                        ui.add_space(15.0);

                                        ui.add(TextEdit::singleline(&mut self.phone1)
                                        .hint_text("Phone Number 1").desired_width(self.widget_size));
                                    
                                        ui.add(TextEdit::singleline(&mut self.phone2)
                                        .hint_text("Phone Number 2").desired_width(self.widget_size));      
                                    });

                                    // Salesman and Tech ComboBoxes
                                    ui.horizontal(|ui|{
                                        ui.add_space(15.0);

                                        ComboBox::from_id_source("salesman_cbox")
                                        .selected_text(format!("{:?}", self.salesman_cbox))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.salesman_cbox, Salesman::Jake, "Jake");
                                            ui.selectable_value(&mut self.salesman_cbox, Salesman::Danny, "Danny");
                                        });

                                        ComboBox::from_id_source("techs_cbox")
                                        .selected_text(format!("{:?}", self.techs_cbox))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.techs_cbox, Techs::Logan, "Logan");
                                            ui.selectable_value(&mut self.techs_cbox, Techs::Bread, "Bread");
                                            ui.selectable_value(&mut self.techs_cbox, Techs::Taco, "Taco");
                                        });
                                    });
                                    ui.horizontal_top(|ui|{
                                        ui.add_space(15.0);
                                        ui.add(TextEdit::multiline(&mut self.checkin_notes)
                                        .hint_text("Checkin Notes").desired_rows(20).desired_width(self.widget_size * 2.0+8.0)); // .desired_width(self.widget_size));
                                    });

                                });
                            });
                        });
                        strip.strip(|builder| {
                            builder.sizes(Size::remainder(), 1).horizontal(|mut strip| {
                                strip.cell(|ui|{
                                    ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,self.strip_bg_color);
                                    ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, self.purple_stroke);

                                });
                            });
                        });
                    });

            }if self.active_tab == 1 {
                ui.vertical_centered(|ui| {
                    ui.columns(2, |cols| {
                        cols[0].add(TextEdit::singleline(&mut self.so_number).hint_text("SO#"));
                        cols[1].add(TextEdit::singleline(&mut self.customer_name).hint_text("Customer Name"));
                    });
                    ui.columns(2, |cols| {
                        cols[0].add(TextEdit::singleline(&mut self.phone1).hint_text("Phone Number 1"));
                        cols[1].add(TextEdit::singleline(&mut self.phone2).hint_text("Phone Number 2"));
                    });
                });
            }
            // Add more tab contents as needed
        });
    }
}