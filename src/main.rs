use app_state::MtechServer;
// use mtechserver_two::MtechServer;
use ratframe::NewCC;
use web_time::Instant;
use std::sync::Arc;
use egui::{Button, CentralPanel, Color32, FontId, Frame, Layout, Style, TopBottomPanel, Vec2};
use egui_aesthetix::{themes::CarlDark, Aesthetix};
use egui_dock::{DockArea, Style as DockStyle};

pub mod tabs;
pub mod app_state;

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    // Redirect `log` message to `console.log` and friends:
    // eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                "the_canvas_id", // hardcode it
                web_options,
                Box::new(|cc| Box::new(MtechServer::new(cc))),
            )
            .await
            .expect("failed to start eframe");
    });
}

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    // env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([300.0, 220.0])
            .with_icon(
                // NOTE: Adding an icon is optional
                eframe::icon_data::from_png_bytes(&include_bytes!("../assets/mtechlogo.png")[..])
                    .expect("Failed to load icon"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "MtechServer",
        native_options,
        Box::new(|cc| Box::new(MtechServer::new(cc))),
    )
}

impl NewCC for MtechServer {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);
        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        // if let Some(storage) = cc.storage {
        //     return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        // }
        MtechServer::default()
    }
    
    fn canvas_id() -> String { "the_canvas_id".into() }
}

impl eframe::App for MtechServer {
    /// Called by the frame work to save state before shutdown.
    // fn save(&mut self, storage: &mut dyn eframe::Storage) { eframe::set_value(storage, eframe::APP_KEY, self); }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = CarlDark;
        let mut custom_style: Style = theme.custom_style();
        let mut font = FontId::default();
        custom_style.spacing.button_padding.x = 2.0;
        custom_style.spacing.button_padding.y = 2.0;
        custom_style.spacing.item_spacing = Vec2::new(5.0, 2.0);
        font.size = 12.0;
        custom_style.override_font_id = Some(font);
        custom_style.spacing.combo_height = 60.0; 
        custom_style.spacing.combo_width = 135.0;
        custom_style.interaction.multi_widget_text_select = false;
        custom_style.interaction.selectable_labels = false;
        custom_style.explanation_tooltips = false;
        custom_style.url_in_tooltip = false;
        let arc_style = Arc::new(custom_style);
        ctx.set_style(arc_style);
        
        let timeout: web_time::Duration = self.context.tick_rate.saturating_sub(self.context.last_tick.elapsed());

        if self.context.last_tick.elapsed() >= self.context.tick_rate {
            self.context.chart_app.on_tick();
            self.context.last_tick = Instant::now();
        }


        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }
                ui.add(Button::new("MasterTech Server"));
                // egui::widgets::global_dark_light_mode_buttons(ui);

                ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| {
                    ui.add(Button::new("Tasks").fill(Color32::from_rgb_additive(255, 12, 180)));

                    ui.add(Button::new("Web Console"));
    
                    ui.add(Button::new("Downloads"));
    
                    ui.add(Button::new("ChatGPT"));
                });

                
            });
        });

        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            eframe::egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &"TUR Sheet".to_string(),
                        &"Scripts".to_string(),
                        &"Console".to_string(),
                        &"System Information".to_string(),
                        &"File Browser 📂".to_string(),
                        &"Minidump Analysis".to_string(),
                        &"Profiler".to_string(),
                        &"QC".to_string(),
                        &"Tasks".to_string(),
                    ] {
                        if ui
                            .selectable_label(self.context.open_tabs.contains(*tab), *tab)
                            .clicked()
                        {
                            if let Some(index) = self.tree.find_tab(&tab.to_string()) {
                                self.tree.remove_tab(index);
                                self.context.open_tabs.remove(*tab);
                            } else {
                                self.tree.push_to_focused_leaf(tab.to_string());
                            }
                            ui.close_menu();
                        }
                    }
                });
            })
        });
        
        CentralPanel::default() // When displaying a DockArea in another UI, it looks better
            .frame(Frame::central_panel(&ctx.style()).inner_margin(4.)) // to set inner margins to 0.
            .show(ctx, |ui| {
                let dock_style = DockStyle::from_egui(ui.style());
                let mut style = self.context.style.get_or_insert(dock_style).clone();
                style.overlay.selection_color = Color32::from_rgb(92,0,87);
                style.separator.color_hovered = Color32::from_rgba_premultiplied(50,93,80,77);
                style.separator.color_idle = Color32::from_rgba_premultiplied(17,17,33,5);
                style.separator.color_dragged = Color32::from_rgba_premultiplied(189,189,189,130);
                style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
                style.main_surface_border_rounding.nw = 15.0;
                style.main_surface_border_rounding.ne = 15.0;
                style.buttons.close_tab_color = Color32::from_rgba_premultiplied(118, 0, 129, 58);
                
                DockArea::new(&mut self.tree)
                    .show_tab_name_on_hover(false)
                    .style(style)
                    .show_close_buttons(self.context.show_close_buttons)
                    .show_add_buttons(self.context.show_add_buttons)
                    .show_add_popup(true)
                    .draggable_tabs(self.context.draggable_tabs)
                    .show_tab_name_on_hover(self.context.show_tab_name_on_hover)
                    .show_inside(ui, &mut self.context);
            });
    }
}


fn setup_custom_fonts(ctx: &egui::Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = egui::FontDefinitions::default();

    // Install my own font (maybe supporting non-latin characters).
    // .ttf and .otf files supported.
    fonts.font_data.insert(
        "Regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Regular.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Regular".into()),
        vec!["Regular".to_owned()],
    );
    fonts.font_data.insert(
        "Bold".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Bold.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Bold".into()),
        vec!["Bold".to_owned()],
    );

    fonts.font_data.insert(
        "Oblique".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Oblique.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Oblique".into()),
        vec!["Oblique".to_owned()],
    );

    fonts.font_data.insert(
        "BoldOblique".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Iosevka-BoldOblique.ttf"
        )),
    );
    fonts.families.insert(
        egui::FontFamily::Name("BoldOblique".into()),
        vec!["BoldOblique".to_owned()],
    );

    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}