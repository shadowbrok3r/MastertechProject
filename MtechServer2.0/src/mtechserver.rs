use app_state::{AppState, MainPages, MtechServer};
use displays::ui_tools::carl_dark::{Aesthetix, CarlDark};
use eframe::egui::{
    style::{HandleShape, NumericColorSpace, Selection, TextCursorStyle, WidgetVisuals, Widgets},
    Color32, Context, CursorIcon, FontFamily, FontId, Frame, Margin, Rounding, Shadow, Stroke,
    Style, Vec2, Visuals, Window,
};
use log::{debug, info};
use std::sync::Arc;
use wasm_bindgen_futures::spawn_local;

use crate::app_state;

impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // most important part of the whole app.. setting up our styling
        // currently this just sets the style of the app, but in the near
        // future i will be making this the setup to allow user customization
        // to the style of any part of the app
        let arc_style = set_darker_style();
        ctx.set_style(arc_style);

        // This is our 'dummy' worker that retrieves Minio bucket storage
        // contents, then builds our 'virtual' file system ui in the
        // crate::tabs::toolbox tab
        // let data_update = self.context.data_update.as_mut().unwrap();
        // if let Some(items) = data_update.take() {
        //     if !items.is_empty() && self.context.file_system.paths.is_empty() {
        //         debug!("Files: {items:?}");
        //         self.context.file_system.build_file_system(items);
        //     }
        // }

        // do some initial setting up
        if self.context.first_run {
            spawn_local(async move {
                gloo_console::info!("Hello from a worker?");
            });
            self.first_run(frame);
        }

        if self.context.wants_to_undock {
            for client in self.context.clients.clone() {
                let undock = if let Some(undock) =
                    self.context.undock_client.get(&client.connection_string)
                {
                    undock
                } else {
                    &false
                };

                if *undock {
                    let color = if client.connected {
                        Color32::LIGHT_BLUE
                    } else {
                        Color32::LIGHT_RED
                    };

                    let column_frame = Frame::default()
                        .fill(Color32::from_rgb(12, 12, 14))
                        .inner_margin(Margin::same(4.0))
                        .outer_margin(Margin::symmetric(5.0, 3.0))
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, color));

                    Window::new(&client.connection_string)
                        .frame(column_frame)
                        .max_size(Vec2::new(700., 400.))
                        .show(ctx, |ui| {
                            ui.vertical_centered_justified(|ui| {
                                ui.horizontal(|ui| self.context.headers(ui, client.clone()));
                                if let Some(ws_client) =
                                    self.context.ws_clients.get_mut(&client.connection_string)
                                {
                                    ws_client.show(ui);
                                }
                            });
                        });
                }
            }
        }

        // Branch out all the different crossbeam channels to receive
        // in their own methods to clean up a lot of boilerplate code
        // as well as being able to find specific code a lot easier
        // self.receive() is the same thing but those crossbeam channels
        // being received have literally one line in them that i dont want to
        // justify creating a separate file / module for
        self.receive();
        self.receive_database(frame);
        self.receive_client();
        self.receive_inventory();
        self.receive_ui_action();
        self.receive_prestashop();
        self.receive_task();
        self.receive_ticket();
        self.receive_notes();
        self.receive_notification();
        self.menu_bar(ctx);
        self.context.handle_modals(ctx);
        self.context.toasts.show(ctx);

        // Get User settings from local storage
        // this bool gets switched via button click
        // in the crate::tabs::json_viewer module
        if self.context.get_settings {
            if let Some(storage) = frame.storage() {
                if let Some(_settings) = storage.get_string("user_settings") {}
            }
        }

        // Get User settings from local storage
        // this bool gets switched via clicking
        // the submit button in the crate::tabs::json_viewer
        // module
        if self.context.update_settings {
            self.context.user_settings.startup_tabs =
                serde_json::to_value(self.tree.clone()).unwrap();

            self.context.update_settings = false;
            info!("Saving settings: {:?}", self.context.user_settings.clone());
            frame.storage_mut().unwrap().set_string(
                "user_settings",
                serde_json::to_string(&self.context.user_settings).unwrap(),
            );
        }

        // Handle changes to state from various places, such as
        // hitting the login button, clicking the 'home page' button
        // (which is clicking Mtechserver in the top middle of the page),
        // if session cookie expires (gets checked in the first_run method),
        // if manually logged out, etc
        match &self.state {
            // Always checking authentication
            AppState::Authenticated(MainPages::Tasks) => self.main_page(ctx),
            AppState::Authenticated(MainPages::Downloads) => self.downloads_page(ctx),
            AppState::Authenticated(MainPages::AccountSettings) => {
                self.account_settings_page(ctx, self.context.app_state_tx.clone())
            }
            AppState::Authenticated(MainPages::WebConsole) => self.web_console(ctx),
            AppState::Authenticated(_) => self.main_page(ctx),
            AppState::CreateAccount => self.signup_page(
                ctx,
                self.context.db_tx.clone(),
                self.context.app_state_tx.clone(),
            ),
            AppState::NoAuth(reason) => {
                if reason.to_string().contains("Already connected") {
                    info!("Already connected");
                    if self.context.current_user.is_some() {
                        self.load_data(frame);
                    } else {
                        self.context.first_run = true;
                        self.first_run(frame)
                    }
                    self.state = AppState::Authenticated(MainPages::Tasks);
                } else {
                    self.login_page(
                        ctx,
                        self.context.db_tx.clone(),
                        self.context.app_state_tx.clone(),
                    )
                }
            }
        }
    }

    fn persist_egui_memory(&self) -> bool {
        true
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self)
    }

    // fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
    //     if let Some(window) = web_sys::window() {
    //         if let Ok(storage) = window.local_storage() {
    //             if let Some(storage) = storage {
    //                 let clear = storage.clear();
    //                 info!("Clearing storage: {clear:?}");
    //             }
    //         }
    //     }
    // }
}

fn set_style() -> Arc<Style> {
    let theme = CarlDark;
    // let theme = TokyoNight;
    let mut custom_style: Style = theme.custom_style();
    let mut font = FontId::default();
    font.size = 10.5;
    font.family = FontFamily::Proportional;

    custom_style.override_font_id = Some(font);
    custom_style.spacing.button_padding.x = 3.0;
    custom_style.spacing.button_padding.y = 3.0;
    custom_style.spacing.item_spacing = Vec2::new(2.0, 1.0);
    custom_style.spacing.combo_height = 55.0;
    custom_style.spacing.combo_width = 100.0;
    custom_style.interaction.multi_widget_text_select = false;
    custom_style.interaction.selectable_labels = true;
    custom_style.explanation_tooltips = false;
    custom_style.url_in_tooltip = true;
    custom_style.interaction.interact_radius = 10.0;
    custom_style.interaction.resize_grab_radius_side = 10.0;
    custom_style.interaction.resize_grab_radius_corner = 10.0;
    custom_style.visuals.window_shadow.spread = 8.0;
    custom_style.visuals.window_shadow.blur = 10.0;
    // custom_style.visuals.panel_fill = Color32::from_rgb(16,16,17);
    // custom_style.visuals.window_fill = Color32::from_rgb(16,16,17);
    custom_style.visuals.selection.stroke.color =
        Color32::from_rgba_premultiplied(199, 20, 150, 100);
    custom_style.visuals.selection.bg_fill = Color32::from_rgba_premultiplied(40, 40, 40, 20);
    custom_style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(17, 17, 19);
    custom_style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    custom_style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(20, 20, 25);
    custom_style.visuals.widgets.inactive.bg_stroke =
        Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
    // custom_style.visuals.widgets.open.bg_fill =  Color32::LIGHT_BLUE;
    // custom_style.visuals.widgets.open.weak_bg_fill =  Color32::LIGHT_BLUE;
    custom_style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(28, 28, 28);
    custom_style.visuals.widgets.active.bg_fill = Color32::LIGHT_GREEN;
    custom_style.visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(15, 15, 19);
    // custom_style.visuals.
    // custom_style.visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
    // custom_style.visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
    custom_style.visuals.widgets.hovered.bg_stroke =
        Stroke::new(0.5, Color32::from_rgba_premultiplied(120, 20, 120, 100));
    let arc_style = Arc::new(custom_style);
    arc_style
}

fn set_darker_style() -> Arc<Style> {
    // Define colors based on "Tokyo Night Dark" theme
    let background_color = Color32::from_rgb(10, 10, 13); // Editor background
    let foreground_color = Color32::from_rgb(169, 177, 214); // Editor foreground
    let widget_bg_color = Color32::from_rgb(20, 20, 22); // Background for inactive widgets
    let hovered_bg_color = Color32::from_rgb(35, 35, 40); // Background for hovered widgets
    let active_bg_color = Color32::from_rgb(28, 28, 28); // Background for active widgets
    let border_color = Color32::from_rgb(16, 16, 23); // Border color for windows and panels
    let text_color = Color32::from_rgb(219, 199, 245); // Color32::from_rgb(199, 202, 245); // Default text color
    let error_color = Color32::from_rgb(227, 104, 176); // Error text color
    let warn_color = Color32::from_rgb(191, 33, 101); // Warning text color
    let link_color = Color32::from_rgb(155, 104, 227); // Hyperlink color

    let theme = CarlDark; // Assuming a theme object or struct
    let mut custom_style: Style = theme.custom_style();

    // Font settings
    let mut font = FontId::default();
    font.size = 10.5;
    font.family = FontFamily::Proportional;

    // Assign custom font
    custom_style.override_font_id = Some(font);

    // Adjust spacing and interactions
    custom_style.spacing.button_padding = Vec2::new(3.0, 3.0);
    custom_style.spacing.item_spacing = Vec2::new(2.0, 1.0);
    custom_style.spacing.combo_height = 55.0;
    custom_style.spacing.combo_width = 100.0;
    custom_style.interaction.selectable_labels = true;
    custom_style.interaction.interact_radius = 10.0;

    // Define visuals with updated values
    custom_style.visuals = Visuals {
        dark_mode: true,                       // Set for dark mode
        override_text_color: Some(text_color), // Global text color override
        widgets: Widgets {
            noninteractive: WidgetVisuals {
                bg_fill: widget_bg_color,
                weak_bg_fill: widget_bg_color,
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(50, 50, 60)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, foreground_color),
                expansion: 0.0,
            },
            inactive: WidgetVisuals {
                bg_fill: widget_bg_color,
                weak_bg_fill: Color32::from_rgb(18, 18, 20),
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(80, 80, 80)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, text_color),
                expansion: 0.0,
            },
            hovered: WidgetVisuals {
                bg_fill: hovered_bg_color,
                weak_bg_fill: Color32::from_rgb(40, 40, 45),
                bg_stroke: Stroke::new(0.5, Color32::from_rgba_premultiplied(120, 20, 120, 100)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, link_color), // Highlight text in link color
                expansion: 0.1,
            },
            active: WidgetVisuals {
                bg_fill: active_bg_color,
                weak_bg_fill: Color32::from_rgb(28, 28, 28),
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(90, 90, 100)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, foreground_color), // Active widget text
                expansion: 0.1,
            },
            open: WidgetVisuals {
                bg_fill: Color32::from_rgb(30, 30, 35),
                weak_bg_fill: Color32::from_rgb(35, 35, 40),
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(100, 100, 110)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, foreground_color), // Open widget text
                expansion: 0.1,
            },
        },
        selection: Selection {
            bg_fill: Color32::from_rgba_premultiplied(90, 55, 88, 90), // Selection background
            stroke: Stroke::new(1.0, Color32::from_rgba_premultiplied(81, 92, 126, 50)), // Selection border
        },
        hyperlink_color: link_color,                   // Hyperlink color
        faint_bg_color: Color32::from_rgb(20, 20, 25), // Subtle background elements
        extreme_bg_color: Color32::from_rgb(15, 15, 20), // Very dark background for contrast
        code_bg_color: Color32::from_rgb(20, 20, 27),  // Background for code blocks
        warn_fg_color: warn_color,                     // Warning text color
        error_fg_color: error_color,                   // Error text color
        window_rounding: Rounding::same(4.0),
        window_shadow: Shadow::default(),
        window_fill: background_color,
        window_stroke: Stroke::new(1.0, border_color), // Window border
        window_highlight_topmost: true,
        menu_rounding: Rounding::same(4.0),
        panel_fill: background_color,
        popup_shadow: Shadow::default(),
        resize_corner_size: 10.0,
        text_cursor: TextCursorStyle::default(),
        clip_rect_margin: 5.0,
        button_frame: true,
        collapsing_header_frame: true,
        indent_has_left_vline: true,
        striped: true,
        slider_trailing_fill: true,
        handle_shape: HandleShape::Circle,
        interact_cursor: Some(CursorIcon::PointingHand),
        image_loading_spinners: true,
        numeric_color_space: NumericColorSpace::Linear, // How numeric values are displayed
    };

    Arc::new(custom_style)
}

fn _set_alternative_style() -> Arc<Style> {
    let theme = CarlDark;
    let mut custom_style: Style = theme.custom_style();
    let mut font = FontId::default();
    font.size = 10.5;
    font.family = FontFamily::Proportional;

    custom_style.override_font_id = Some(font);
    custom_style.spacing.button_padding.x = 3.0;
    custom_style.spacing.button_padding.y = 3.0;
    custom_style.spacing.item_spacing = Vec2::new(2.0, 1.0);
    custom_style.spacing.combo_height = 55.0;
    custom_style.spacing.combo_width = 100.0;
    custom_style.interaction.multi_widget_text_select = false;
    custom_style.interaction.selectable_labels = false;
    custom_style.explanation_tooltips = false;
    custom_style.url_in_tooltip = true;
    custom_style.interaction.interact_radius = 10.0;
    custom_style.interaction.resize_grab_radius_side = 10.0;
    custom_style.interaction.resize_grab_radius_corner = 10.0;
    custom_style.visuals.window_shadow.spread = 8.0;
    custom_style.visuals.window_shadow.blur = 10.0;

    // Update color scheme based on the extracted colors
    custom_style.visuals.selection.stroke.color = Color32::from_rgb(199, 20, 150); // Kept the same for contrast
    custom_style.visuals.selection.bg_fill = Color32::from_rgb(40, 40, 40); // Kept the same for contrast
    custom_style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(13, 16, 23);
    custom_style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    custom_style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(12, 15, 22);
    custom_style.visuals.widgets.inactive.bg_stroke =
        Stroke::new(1.0, Color32::from_rgb(21, 24, 31));
    custom_style.visuals.widgets.open.bg_fill = Color32::from_rgb(18, 21, 28);
    custom_style.visuals.widgets.open.weak_bg_fill = Color32::from_rgb(18, 21, 28);
    custom_style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(18, 21, 28);
    custom_style.visuals.widgets.active.bg_fill = Color32::from_rgb(20, 23, 29);
    custom_style.visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(12, 15, 22);
    custom_style.visuals.widgets.hovered.bg_stroke =
        Stroke::new(0.5, Color32::from_rgb(199, 20, 150)); // Kept the same for contrast

    let arc_style = Arc::new(custom_style);
    arc_style
}
