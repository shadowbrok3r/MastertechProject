#![allow(deprecated)]
use eframe::egui::{vec2, Align, Button, CentralPanel, Color32, ComboBox, Direction, FontId, Frame, Id, InnerResponse, Key, Layout, PopupCloseBehavior, Rect, RichText, ScrollArea, TextEdit, Ui, UiBuilder, Vec2, Widget};
use database::{schema::{McpSettings, SortDirection, Status, Store, User}, DatabaseSelection, PlatformSpawner, Spawner, SurrealValue, db};
use crate::{app_state::{AppState, MainPages, SharedContext}, tabs::tasks::task_layout::{SortField, SortOptions}, ui_tools::icons};
use egui_extras::{Size, StripBuilder};
use crossbeam::channel::Sender;
use serde::Serialize;
use log::{error, info};

#[derive(Serialize, Debug, Clone, Default, database::SurrealValue)]
pub struct UserPreferences {
    name: String,
    email: String,
    password: String,
    retyped_password: String,
    store: Store,
    database: DatabaseSelection,
    sort_by: SortOptions,
    last_sort_field: Option<SortField>,
    new_status: String,
    mcp_endpoint: String,
    mcp_api_key: String,
    mcp_model: String,
    user: User
}

impl UserPreferences {
    pub fn set_user(&mut self, user: User) {
        self.mcp_endpoint = user.get_mcp_endpoint().unwrap_or_default();
        self.mcp_api_key = user.get_mcp_api_key().unwrap_or_default();
        self.mcp_model = user.get_mcp_model().unwrap_or_default();
        self.user = user.clone();
    }

    pub fn save_mcp_settings(&mut self) {
        let to_opt = |s: &str| { let t = s.trim(); if t.is_empty() { None } else { Some(t.to_string()) } };
        let settings = McpSettings {
            endpoint: to_opt(&self.mcp_endpoint),
            api_key: to_opt(&self.mcp_api_key),
            model: to_opt(&self.mcp_model),
        };
        self.user.set_mcp_settings(settings.clone());
        crate::ai::apply_mcp_settings(&self.user);
        PlatformSpawner::spawn(async move {
            if let Err(e) = User::save_mcp_settings(settings).await {
                error!("Error saving mcp settings: {e:?}");
            }
        });
    }

    pub async fn mod_account(&self, user_id: String) {
        let acc_mod: UserPreferences = Self {
            name: self.name.clone(),
            email: self.email.clone(),
            password: self.password.clone(),
            ..Default::default()
        };

        let mod_user_result: Result<_, surrealdb::Error> = db()
            .query("fn::modify_account($user, $new)")
            .bind(("user", user_id))
            .bind(("new", acc_mod))
            .await;

        info!("mod_user_result: {mod_user_result:?}"); 
    }

    pub fn change_password(&self) {
        let password = self.password.clone();
        PlatformSpawner::spawn(async move {
            let x: Result<_, surrealdb::Error> = db()
                .query("UPDATE $auth.id SET password = crypto::argon2::generate($pass)")
                .bind(("pass", password))
                .await;
            info!("X: {x:?}");
        });
    }
}

impl SharedContext {
    pub fn account_settings_page(&mut self, ui: &mut eframe::egui::Ui, appstate_tx: Sender<AppState>) {
        CentralPanel::default()
            .frame(Frame::central_panel(&ui.ctx().global_style()).inner_margin(1.))
            .show(ui, |ui|
        {
            StripBuilder::new(ui)
                .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
                .size(Size::initial(300.0).at_least(200.).at_most(500.0))
                .size(Size::remainder())
                .size(Size::initial(300.0).at_least(200.).at_most(500.0))
                .horizontal(|mut s| {
                    s.empty();
                    s.strip(|s| 
                    {
                        s
                            .cell_layout(Layout::left_to_right(Align::Center))
                            .size(Size::exact(50.0))
                            .size(Size::remainder())
                            .size(Size::exact(50.0))
                            .vertical(|mut s| 
                        {
                            s.empty();
                            s.cell(|ui| 
                            {
                                ui.vertical_centered(|ui| {
                                    let font = FontId::monospace(15.0);
                                    ui.style_mut().override_font_id = Some(font);
                                    
                                    let avail_size = vec2(ui.available_width()/3.2, 222.);

                                    ui.horizontal_top(|ui| {
                                        ui.set_min_width(avail_size.x);
                                        ui.columns(3, |ui| {
                                            ui[0].vertical_centered(|ui| {
                                                ui.heading(RichText::new("Account Settings").strong());
                                                ui.add_space(5.0);
                                                ui.group(|ui| {
                                                    ui.set_min_size(avail_size);
                                                    if let (
                                                        Some(ref mut usr), 
                                                        Some(acc_mod)
                                                    ) = (
                                                        self.current_user.clone(), 
                                                        self.account_mut()
                                                    ){
                                                        TextEdit::singleline(&mut usr.get_name())
                                                            .hint_text(" Name")
                                                            .vertical_align(Align::Center)
                                                            .desired_width(230.)
                                                            .ui(ui);

                                                        ui.add_space(5.0);

                                                        TextEdit::singleline(&mut usr.get_email())
                                                            .hint_text(" Email")
                                                            .vertical_align(Align::Center)
                                                            .desired_width(230.)
                                                            .ui(ui);

                                                        ui.add_space(5.0);

                                                        ui.scope_builder(
                                                        UiBuilder::new()
                                                        .layout(Layout::from_main_dir_and_cross_align(Direction::LeftToRight, Align::Min)), 
                                                        |ui| {
                                                            let is_sizing_pass = ui.is_sizing_pass();
                                                            let available_width = ui.available_width();
                                                            let combo_width = 115.0; // Fixed width of each ComboBox
                                                            let total_content_width = combo_width * 2.0; // Two ComboBoxes side by side

                                                            // In the rendering pass, add spacing to center the content
                                                            if !is_sizing_pass && available_width > total_content_width {
                                                                let padding = (available_width - total_content_width) / 2.0;
                                                                ui.add_space(padding);
                                                            }
                                                            ComboBox::new("StoreComboBox", "")
                                                                .selected_text(acc_mod.store.as_str())
                                                                .width(115.)
                                                                .show_ui(ui, |ui| 
                                                            {
                                                                for store in Store::VALUES {
                                                                    ui.selectable_value(&mut acc_mod.store, store, store.as_str());
                                                                }
                                                            });

                                                            // ui.add_space(width);
                                                            let db = ComboBox::new("Database Editor", "")
                                                                .selected_text(format!("{:?}", acc_mod.database))
                                                                .width(115.)
                                                                .show_ui(ui, |ui| 
                                                            {
                                                                ui.selectable_value(
                                                                    &mut acc_mod.database, 
                                                                    DatabaseSelection::Stable, 
                                                                    DatabaseSelection::Stable.as_str()
                                                                );
                                                                ui.selectable_value(
                                                                    &mut acc_mod.database, 
                                                                    DatabaseSelection::Beta, 
                                                                    DatabaseSelection::Beta.as_str()
                                                                );
                                                                ui.selectable_value(
                                                                    &mut acc_mod.database, 
                                                                    DatabaseSelection::Local, 
                                                                    DatabaseSelection::Local.as_str()
                                                                );
                                                            });
                                                            if db.response.clicked(){
                                                                let acc = acc_mod.clone();
                                                                PlatformSpawner::spawn(async move {
                                                                    let set_db = acc.database.set_database().await;
                                                                    log::info!("Set database: {set_db:?}");
                                                                });
                                                            }
                                                            // Use egui memory to track if we've already done the sizing pass
                                                            let sizing_pass_done = ui.memory(|mem| mem.data.get_temp::<bool>(Id::new("combo_sizing_done")).unwrap_or(false));

                                                            if is_sizing_pass && !sizing_pass_done {
                                                                ui.ctx().request_discard("Centering ComboBox sizing pass");
                                                                ui.ctx().request_repaint();
                                                                ui.memory_mut(|mem| mem.data.insert_temp(Id::new("combo_sizing_done"), true));
                                                            }

                                                            // Reset the flag if the UI is repainted for other reasons (e.g., window resize)
                                                            if !is_sizing_pass && sizing_pass_done {
                                                                ui.memory_mut(|mem| mem.data.insert_temp(Id::new("combo_sizing_done"), false));
                                                            }
                                                        });

                                                        ui.add_space(5.0);
                                                        
                                                        TextEdit::singleline(&mut acc_mod.password)
                                                            .hint_text(" Password")
                                                            .desired_width(230.0)
                                                            .password(true)
                                                            .ui(ui);

                                                        ui.add_space(5.0);
                                                        let password_check = acc_mod.password.eq(&acc_mod.retyped_password.clone());
                                                        let background_color = if password_check {
                                                            ui.global_style().visuals.extreme_bg_color
                                                        } else {
                                                            ui.global_style().visuals.error_fg_color
                                                        };

                                                        TextEdit::singleline(&mut acc_mod.retyped_password)
                                                            .background_color(background_color)
                                                            .hint_text(" Retype Password")
                                                            .desired_width(230.0)
                                                            .password(true)
                                                            .ui(ui);

                                                        ui.add_space(5.0);

                                                        let change_pw_txt = if password_check {" Change Password "} else { " Passwords must match " };
                                                        
                                                        let button = Button::new(change_pw_txt)
                                                            .fill(Color32::from_rgb(30, 30, 35))
                                                            .min_size(Vec2::new(140.0, 15.0));

                                                        let enabled_button = ui.add_enabled(acc_mod.password.len() > 3 && password_check, button);
                                                        let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

                                                        if enabled_button.clicked() || accepted_by_keyboard {
                                                            acc_mod.change_password();
                                                            let tx = appstate_tx.clone();
                                                            match tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                                                                Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                                                Err(e) => error!("Error {e:?}"),
                                                            }
                                                        }
                                                    }
                                                });
                                            });
                                            ui[1].vertical_centered(|ui| {
                                                ui.heading(RichText::new("Task Page Preferences").strong());
                                                ui.add_space(5.0);
                                                ui.group(|ui| {
                                                    ui.set_min_size(avail_size);
                                                    
                                                    ui.horizontal(|ui| {
                                                        ui.label("Custom Status: ");
                                                        
                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            let submit_status = ui.small_button("✔️");
                                                            TextEdit::singleline(&mut self.account_mod.new_status).desired_width(115.).ui(ui);

                                                            if submit_status.clicked() && !self.account_mod.new_status.is_empty() {
                                                                let status = self.account_mod.new_status.clone();
                                                                self.account_mod.new_status.clear();
                                                                self.account_mod.user.set_statuses(Status::CustomStatus(status.clone()));
                                                                log::info!("Got a new status: {status}");
                                                                PlatformSpawner::spawn(async move {
                                                                    match User::add_custom_status(status.clone().as_str()).await {
                                                                        Ok(_) => log::info!("Created new status: {}", status),
                                                                        Err(e) => log::error!("Error creating new status: {e:?}")
                                                                    }
                                                                });
                                                            }
                                                        });
                                                    });
                                                    
                                                    ui.add_space(5.0);
                                                    
                                                    ui.horizontal(|ui| {
                                                        ui.label("Default Priority: ");
                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            let mut selected = self.account_mod.sort_by.clone();
                                                            let txt = match selected.direction {
                                                                SortDirection::Asc => ("↗", ui.global_style().visuals.warn_fg_color),
                                                                SortDirection::Desc => ("↘", ui.global_style().visuals.error_fg_color),
                                                            };
                                                            let selected_text = match selected.field {
                                                                SortField::Default => RichText::new(format!("Priority {}", txt.0)).color(txt.1).small(),
                                                                SortField::Date => RichText::new(format!("Date {}", txt.0)).color(txt.1).small(),
                                                                SortField::Name => RichText::new(format!("Name {}", txt.0)).color(txt.1).small(),
                                                            };
                                                            
                                                            ComboBox::new(format!("SortBy Settings"), "")
                                                            .selected_text(selected_text)
                                                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                                                            .width(120.)
                                                            .show_ui(ui, |ui| {
                                                                if ui.selectable_value(
                                                                    &mut selected.field, 
                                                                    SortField::Default, 
                                                                    RichText::new(format!("Priority {}", txt.0)).color(txt.1).small())
                                                                .clicked() {
                                                                    if let Some(last_field) = self.account_mod.last_sort_field.clone() {
                                                                        if last_field == SortField::Default {
                                                                            // Toggle the direction if the same field is clicked again
                                                                            selected.direction = match selected.direction {
                                                                                SortDirection::Asc => SortDirection::Desc,
                                                                                SortDirection::Desc => SortDirection::Asc,
                                                                            };
                                                                        }
                                                                    }
                                                                    // Update the last selected field
                                                                    self.account_mod.last_sort_field = Some(SortField::Default);
                                                                }
                                                                if ui.selectable_value(
                                                                    &mut selected.field, 
                                                                    SortField::Name, 
                                                                    RichText::new(format!("Name {}", txt.0)).color(txt.1).small())
                                                                .clicked() {
                                                                    if let Some(last_field) = self.account_mod.last_sort_field.clone() {
                                                                        if last_field == SortField::Name {
                                                                            // Toggle the direction if the same field is clicked again
                                                                            selected.direction = match selected.direction {
                                                                                SortDirection::Asc => SortDirection::Desc,
                                                                                SortDirection::Desc => SortDirection::Asc,
                                                                            };
                                                                        }
                                                                    }
                                                                    // Update the last selected field
                                                                    self.account_mod.last_sort_field = Some(SortField::Name);
                                                                }
                                                                if ui.selectable_value(
                                                                    &mut selected.field, 
                                                                    SortField::Date, 
                                                                    RichText::new(format!("Date {}", txt.0)).color(txt.1).small())
                                                                .clicked() {
                                                                    if let Some(last_field) = self.account_mod.last_sort_field.clone() {
                                                                        if last_field == SortField::Date {
                                                                            // Toggle the direction if the same field is clicked again
                                                                            selected.direction = match selected.direction {
                                                                                SortDirection::Asc => SortDirection::Desc,
                                                                                SortDirection::Desc => SortDirection::Asc,
                                                                            };
                                                                        }
                                                                    }
                                                                    // Update the last selected field
                                                                    self.account_mod.last_sort_field = Some(SortField::Date);
                                                                }
                                                            });
                                                        });
                                                    });

                                                    ScrollArea::vertical()
                                                    .auto_shrink([false, false])
                                                    .max_width(160.)
                                                    .max_height(avail_size.y - 70.)
                                                    .show(ui, |ui| {
                                                        let clicked: &mut Option<Status> = &mut None;
                                                        for (idx, status) in self.account_mod.user.get_custom_statuses().iter().enumerate() {
                                                            ui.horizontal(|ui| {
                                                                ui.label(format!("{idx}: "));
                                                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                                    if ui.button("❌").clicked() {
                                                                        *clicked = Some(status.clone());
                                                                        let status = status.clone();
                                                                        PlatformSpawner::spawn(async move {
                                                                            match User::remove_custom_status(status.as_str()).await {
                                                                                Ok(_) => log::info!("Deleted status: {:?}", status),
                                                                                Err(e) => log::error!("Error deleting status: {e:?}")
                                                                            }
                                                                        });
                                                                    }
                                                                    ui.add_space(5.);
                                                                    ui.label(status.as_str());
                                                                });
                                                            });
                                                        }
                                                        if let Some(status) = clicked {
                                                            if let Some(statuses) = self.account_mod.user.get_custom_statuses_mut() {
                                                                let index = statuses.iter().position(|x| *x == *status);
                                                                if let Some(idx) = index {
                                                                    statuses.remove(idx);
                                                                }
                                                            }
                                                        }
                                                    });
                                                });
                                            });
                                            ui[2].vertical_centered(|ui| {
                                                ui.heading(RichText::new("Other Account Details").strong());
                                                ui.add_space(5.0);
                                                ui.group(|ui| {
                                                    ui.set_min_size(avail_size);
                                                    ui.horizontal(|ui| ui.label("Minio Access Key: "));
                                                    ui.horizontal(|ui| ui.colored_label(ui.global_style().visuals.error_fg_color, self.account_mod.user.get_minio_access_key().unwrap_or_default()));

                                                    ui.add_space(5.0);

                                                    ui.horizontal(|ui| ui.label("Minio Secret Key: "));
                                                    ui.scope(|ui| {
                                                        ui.style_mut().override_font_id = Some(FontId::monospace(12.));
                                                        ui.horizontal(|ui| ui.colored_label(ui.global_style().visuals.error_fg_color, self.account_mod.user.get_minio_secret_key().unwrap_or_default()));
                                                    });
                                                    
                                                    ui.add_space(5.0);

                                                    ui.horizontal(|ui| {
                                                        ui.label("Prestashop ID: ");
                                                        
                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            ui.label(self.account_mod.user.get_employee_id().unwrap_or_default().to_string());
                                                        });
                                                    });

                                                    ui.add_space(5.0);

                                                    ui.horizontal(|ui| {
                                                        ui.label("Store #: ");
                                                        
                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            ui.label(self.account_mod.user.get_store_id().unwrap_or_default());
                                                        });
                                                    });

                                                    ui.add_space(5.0);

                                                    ui.horizontal(|ui| {
                                                        ui.label("Authorization: ");
                                                        
                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            ui.label(self.account_mod.user.get_authorization().as_str());
                                                        });
                                                    });

                                                    ui.add_space(5.0);

                                                    ui.horizontal(|ui| {
                                                        ui.label("MtechServer Version: ");
                                                        
                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            ui.label(self.account_mod.user.get_version());
                                                        });
                                                    });
                                                    // ui.horizontal(|ui| {
                                                    //     ui.label("Database Version: ");
                                                        
                                                    //     ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                    //         ui.label(db().version());
                                                    //     });
                                                    // });
                                                });
                                            });
                                        });
                                    });

                                    ui.add_space(10.0);
                                    ui.heading(RichText::new("MCP / AI Endpoint").strong());
                                    ui.add_space(10.0);

                                    ui.group(|ui| {
                                        ui.set_max_width(360.0);
                                        ui.vertical_centered(|ui| {
                                            TextEdit::singleline(&mut self.account_mod.mcp_endpoint)
                                                .hint_text(" OpenAI-compatible Endpoint")
                                                .desired_width(330.)
                                                .ui(ui);

                                            ui.add_space(5.0);

                                            TextEdit::singleline(&mut self.account_mod.mcp_api_key)
                                                .hint_text(" API Key")
                                                .desired_width(330.)
                                                .password(true)
                                                .ui(ui);

                                            ui.add_space(5.0);

                                            TextEdit::singleline(&mut self.account_mod.mcp_model)
                                                .hint_text(" Model")
                                                .desired_width(330.)
                                                .ui(ui);

                                            ui.add_space(8.0);

                                            let save = Button::new(format!("{}  Save Endpoint", icons::SAVE))
                                                .fill(Color32::from_rgb(30, 30, 35))
                                                .min_size(Vec2::new(160.0, 18.0));

                                            if ui.add(save).clicked() {
                                                self.account_mod.save_mcp_settings();
                                            }
                                        });
                                    });
                                });
                            });
                            s.empty();
                        });
                    });
                    s.empty();
                });
        });
    }
}

/// A helper function to center arbitrary UI content horizontally.
/// Takes a closure that defines the content to be centered and returns its response.
pub fn centered_ui<R>(
    ui: &mut Ui,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let id = ui.make_persistent_id("centered_ui");
    // Check if this is the first frame by seeing if the Id exists in memory
    let is_first_frame = ui.memory(|mem| !mem.data.get_temp(id).unwrap_or(false));

    // Step 1: Set up the UiBuilder
    let max_rect = ui.cursor().intersect(ui.max_rect());
    let mut ui_builder = UiBuilder::new()
        .max_rect(max_rect)
        .layout(Layout::from_main_dir_and_cross_align(Direction::LeftToRight, Align::Min));

    if is_first_frame {
        // Perform a sizing pass and hide the initial frame to avoid glitches
        if ui.is_visible() {
            ui.ctx().request_discard("Initial sizing pass for centered_ui");
        }
        ui_builder = ui_builder.sizing_pass().invisible();
    }

    // Step 2: Use scope_builder to create the child Ui and render the content
    ui.scope_builder(ui_builder, |ui| {
        let is_sizing_pass = ui.is_sizing_pass();

        // Measure the content width
        let content_response = add_contents(ui);
        let content_rect: Rect = ui.min_rect();
        let content_width = content_rect.width();

        // In the rendering pass, add padding to center the content
        if !is_sizing_pass {
            let available_width = ui.available_width();
            if available_width > content_width {
                let padding = (available_width - content_width) / 2.0;
                ui.add_space(padding);
            }
        }

        // Mark that we've completed the first frame
        if is_first_frame {
            ui.memory_mut(|mem| mem.data.insert_temp(id, true));
        }

        content_response
    })
}