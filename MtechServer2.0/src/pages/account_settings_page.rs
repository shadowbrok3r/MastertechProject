use displays::{tasks::task_layout::{SortField, SortOptions}, SortDirection};
use eframe::egui::{vec2, Align, Button, CentralPanel, Color32, ComboBox, Context, Direction, FontId, Frame, Id, InnerResponse, Key, Layout, PopupCloseBehavior, Rect, RichText, TextEdit, Ui, UiBuilder, Vec2, Widget};
use crate::app_state::{AppState, MainPages, MtechServer};
use database::{schema::{Store, User}, DatabaseSelection, PlatformSpawner, Spawner, DATABASE};
use egui_extras::{Size, StripBuilder};
use crossbeam::channel::Sender;
use serde::Serialize;
use log::{error, info};

#[derive(Serialize, Debug, Default, Clone)]
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
}

impl UserPreferences{
    pub async fn mod_account(&self, user_id: String) {
        let acc_mod: UserPreferences = Self {
            name: self.name.clone(),
            email: self.email.clone(),
            password: self.password.clone(),
            ..Default::default()
        };

        let mod_user_result: Result<surrealdb::Response, surrealdb::Error> = DATABASE
            .query("fn::modify_account($user, $new)")
            .bind(("user", user_id))
            .bind(("new", acc_mod))
            .await;

        info!("mod_user_result: {mod_user_result:?}"); 
    }

    pub async fn change_password(&self) {
        let password = self.password.clone();
        let x: Result<surrealdb::Response, surrealdb::Error> = DATABASE
            .query("UPDATE user SET password = crypto::argon2::generate($pass) WHERE id == $auth.id")
            .bind(("pass", password))
            .await;
        info!("X: {x:?}");
    }
}

impl MtechServer{
    pub fn account_settings_page(&mut self, ctx: &Context, appstate_tx: Sender<AppState>) {
        CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(1.))
            .show(ctx, |ui| 
        {
            StripBuilder::new(ui)
                .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
                .size(Size::exact(500.0))
                .size(Size::remainder())
                .size(Size::exact(500.0))
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
                                    let font = FontId::proportional(15.0);
                                    ui.style_mut().override_font_id = Some(font);
                                    
                                    let avail_size = vec2(ui.available_width()/3.2, 222.);

                                    ui.horizontal_top(|ui| {
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
                                                        self.context.shared_ctx.current_user.clone(), 
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
                                                                    acc.database.set_database().await;
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
                                                            ui.style().visuals.extreme_bg_color
                                                        } else {
                                                            ui.style().visuals.error_fg_color
                                                        };

                                                        TextEdit::singleline(&mut acc_mod.retyped_password)
                                                            .background_color(background_color)
                                                            .hint_text(" Retype Password")
                                                            .desired_width(230.0)
                                                            .password(true)
                                                            .ui(ui);

                                                        ui.add_space(5.0);

                                                        if Button::new("Update Account")
                                                            .fill(Color32::from_rgb(30, 30, 35))
                                                            .min_size(Vec2::new(140.0, 15.0))
                                                            .ui(ui)
                                                            .clicked()
                                                        {
                                                            let email = if usr.get_email().ends_with("@pclaptops.com"){
                                                                usr.get_email().to_owned()
                                                            } else {
                                                                format!("{}@pclaptops.com", usr.get_email())
                                                            };

                                                            let acc_mod = UserPreferences {
                                                                email,
                                                                name: usr.get_name().to_string(),
                                                                store: acc_mod.store,
                                                                ..Default::default()
                                                            };

                                                            info!("Account Mod: {:?}", acc_mod);
                                                            let tx = appstate_tx.clone();
                                                            let user = usr.get_id().key().to_string().clone();
                                                            PlatformSpawner::spawn(async move {
                                                                acc_mod.mod_account(user).await;
                                                                match tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                                                                    Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                                                    Err(e) => error!("Error {e:?}"),
                                                                }
                                                            });
                                                        }
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
                                                            let accepted_by_keyboard = ui.ctx().input_mut(|i| i.key_pressed(eframe::egui::Key::Enter));
                                                            let res = TextEdit::singleline(&mut self.account_mod.new_status).desired_width(115.).show(ui);
                                                            if ( accepted_by_keyboard || res.response.lost_focus() ) && !self.account_mod.new_status.is_empty() {
                                                                let status = self.account_mod.new_status.clone();
                                                                self.account_mod.new_status.clear();
                                                                log::info!("Got a new status: {status}");
                                                                PlatformSpawner::spawn(async move {
                                                                    match User::add_custom_status(database::schema::Status::CustomStatus(status.clone())).await {
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
                                                                SortDirection::Asc => ("↗", ui.style().visuals.warn_fg_color),
                                                                SortDirection::Desc => ("↘", ui.style().visuals.error_fg_color),
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
                                                });
                                            });
                                            ui[2].vertical_centered(|ui| {
                                                ui.heading(RichText::new("Other").strong());
                                                ui.add_space(5.0);
                                                ui.group(|ui| {
                                                    ui.set_min_size(avail_size);
                                                });
                                            });
                                        });
                                    });

                                    ui.add_space(10.0);
                                    ui.heading("Theme Configuration");
                                    ui.add_space(10.0);

                                    ui.group(|ui| {
                                        let tx = self.context.shared_ctx.settings_sender.clone();
                                        self.context.shared_ctx.theme_config.edit_ui(ui, tx);
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