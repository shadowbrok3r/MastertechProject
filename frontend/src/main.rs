// #[war(unused_imports)]
use app_state::MtechServer;
use crossbeam::channel::Sender;
use database::{schema::Store, Database};
use ratframe::NewCC;
use utilities::listen_tasks::{get_completed_tasks, get_my_tasks, get_store_tasks};
use wasm_bindgen_futures::spawn_local;
use web_time::Instant;
use std::sync::Arc;
use egui::{Button, CentralPanel, Color32, FontId, Frame, Layout, Style, TopBottomPanel, Vec2};
use egui_aesthetix::{themes::CarlDark, Aesthetix};
use egui_dock::{DockArea, Style as DockStyle};
use log::{LevelFilter, debug, info};

pub mod tabs;
pub mod app_state;
pub mod utilities;
pub mod webworker;

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    eframe::WebLogger::init(LevelFilter::Debug).ok();
    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                "mtech_canvas", // hardcode it
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

impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // setup our styling for the site
        let arc_style = set_style();
        ctx.set_style(arc_style);
        
        // i have no god damn idea what this is really doing. it was a 
        // wasm example for using web workers.. i dont even know if its required???
        let data_update = self.context.data_update.as_mut().unwrap();
        if let Some(update) = data_update.take() {
            log::debug!("Received update: {update:?}")
        }

        // For updating our Ratatui chart in the RataGuiBackend terminal
        if self.context.last_tick.elapsed() >= self.context.tick_rate {
            self.context.chart_app.on_tick();
            self.context.last_tick = Instant::now();
        }

        // do some setting up in the initial frame of our update loop for 
        // 1. Getting database connection
        if self.context.first_run{
            self.context.first_run = false;
            let db_tx = self.context.db_tx.clone();
            first_run_data(db_tx, ctx);
        }

        // Retrieve our database connection, and 
        // 2. Requesting some task data
        if let Ok(db) = self.context.db_rx.try_recv(){
            self.context.database = Some(db.clone());

            let my_tasks_tx = self.context.my_tasks_tx.clone();
            let store_tasks_tx = self.context.store_tasks_tx.clone();
            let completed_tasks_tx = self.context.completed_tasks_tx.clone();

            get_my_tasks(db.clone(), my_tasks_tx, "LL".to_string());
            get_store_tasks(db.clone(), store_tasks_tx, Store::RIV);
            get_completed_tasks(db.clone(), completed_tasks_tx, Store::RIV);
        }

        if let Ok(tasks) = self.context.my_tasks_rx.try_recv(){
            self.context.my_tasks = Some(tasks);
        }

        if let Ok(tasks) = self.context.store_tasks_rx.try_recv(){
            self.context.store_tasks = Some(tasks);
        }

        if let Ok(tasks) = self.context.completed_tasks_rx.try_recv(){
            self.context.completed_tasks = Some(tasks);
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| 
        {
            egui::menu::bar(ui, |ui| {
                ui.add(Button::new("MasterTech Server"));
                ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| {
                    ui.add(Button::new("Store Tasks").fill(Color32::from_rgb_additive(255, 12, 180)));
                    ui.add(Button::new("Web Console"));
                    ui.add(Button::new("Downloads"));
                    ui.add(Button::new("ChatGPT"));
                });
            });
        });

        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| 
        {
            eframe::egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &"Store Tasks".to_string(),
                        &"My Tasks".to_string(),
                        &"Terminal".to_string(),
                        &"Web Console".to_string(),
                        &"Completed Tasks".to_string()
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
        
        CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(1.))
            .show(ctx, |ui| 
        {
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
                    .style(style)
                    .show_close_buttons(true)
                    .show_add_buttons(true)
                    .show_add_popup(true)
                    .draggable_tabs(true)
                    .show_inside(ui, &mut self.context);
        });
    }

    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) { 
        // eframe::set_value(storage, eframe::APP_KEY, self); 
    }
}


fn set_style() -> Arc<Style>{
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
    arc_style
}

fn first_run_data(db_tx: Sender<Database>, ctx: &egui::Context) {
    egui::Window::new("load_database_spin")
        .anchor(egui::Align2::RIGHT_TOP, egui::Vec2::ZERO)
        .title_bar(false)
        .enabled(false)
        .auto_sized()
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading Database");
            })
        });
    
    info!("First run / Spawning local to get database");

    spawn_local(async move {
        let database = Database::new().await;
        match db_tx.send(database){
            Ok(_) => debug!("Sent db connection across thread"),
            Err(err) => debug!("Error sending db connection: {err:?}"),
        }
    });
}