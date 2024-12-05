use eframe::egui::{Align, Button, CentralPanel, Color32, ComboBox, Context, Direction, FontId, Frame, Layout, RichText, TextEdit, Vec2, Widget};
use database::{schema::Store, set_db_selection, DatabaseSelection, DATABASE};
use crate::app_state::{AppState, MainPages, MtechServer};
use surrealdb::Response;
use egui_extras::{Size, StripBuilder};
use wasm_bindgen_futures::spawn_local;
use crossbeam::channel::Sender;
use serde::Serialize;
use log::{error, info};

#[derive(Serialize, Debug, Default, Clone)]
pub struct AccountMod {
    pub name: String,
    pub email: String,
    pub password: String,
    pub store: Store,
    pub database: DatabaseSelection,
}

impl AccountMod{
    pub fn mod_account(&self, _appstate_tx: Sender<AppState>, user_id: String){
        let acc_mod: AccountMod = Self {
            name: self.name.clone(),
            email: self.email.clone(),
            store: self.store.clone(),
            database: self.database.clone(),
            password: self.password.clone()
        };

        spawn_local(async move {
            let mod_user_result: Result<Response, surrealdb::Error> = DATABASE
                .query("fn::modify_account($user, $new)")
                .bind(("user", user_id))
                .bind(("new", acc_mod))
                .await;
            info!("mod_user_result: {mod_user_result:?}");
        });
    }

    pub fn change_password(&self, user_id: String){
        let password = self.password.clone();
        spawn_local(async move {
            let x: Result<surrealdb::Response, surrealdb::Error> = DATABASE
                .query("UPDATE $usr SET password = crypto::argon2::generate($pass)")
                .bind(("usr", user_id))
                .bind(("pass", password))
                .await;
            info!("X: {x:?}");
        });
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
                .sizes(Size::remainder(), 3)
                .horizontal(|mut s| {
                    s.empty();
                    s.strip(|s| 
                    {
                        s
                            .cell_layout(Layout::centered_and_justified(Direction::TopDown))
                            .size(Size::exact(150.0))
                            .size(Size::remainder())
                            .size(Size::exact(100.0))
                            .vertical(|mut s| 
                        {
                            s.empty();
                            s.cell(|ui| 
                            {
                                ui.group(|ui| 
                                { 
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(100.0);
                                        ui.label(RichText::new("Modify Account").heading());
                                        let font = FontId::proportional(15.0);
                                        ui.style_mut().override_font_id = Some(font);
        
                                        ui.add_space(20.0);

                                        if let (Some(ref mut usr), Some(acc_mod)) = (self.context.shared_ctx.current_user.clone(), self.account_mut()){
                                            let width = ui.available_width() / 3.0 + 10.0;

                                                TextEdit::singleline(&mut usr.name)
                                                    .hint_text("Name")
                                                    .desired_width(180.0)
                                                    .ui(ui);

                                                ui.add_space(5.0);

                                                TextEdit::singleline(&mut usr.email)
                                                    .hint_text("Email")
                                                    .desired_width(180.0)
                                                    .ui(ui);

                                            ui.add_space(5.0);

                                            ui.horizontal_top(|ui| {
                                                ui.add_space(width);
                                                ComboBox::new("StoreComboBox", "")
                                                    .selected_text(acc_mod.store.as_str())
                                                    .width(180.0)
                                                    .show_ui(ui, |ui| 
                                                {
                                                    for store in Store::VALUES {
                                                        ui.selectable_value(&mut acc_mod.store, store, store.as_str());
                                                    }
                                                });
                                            });

                                            ui.horizontal_top(|ui| {
                                                ui.add_space(width);
                                                let db = ComboBox::new("Database", "")
                                                    .selected_text(format!("{:?}", acc_mod.database))
                                                    .width(180.0)
                                                    .show_ui(ui, |ui| 
                                                {
                                                    ui.selectable_value(&mut acc_mod.database, DatabaseSelection::Stable, "Stable");
                                                    ui.selectable_value(&mut acc_mod.database, DatabaseSelection::Beta, "Beta");
                                                });
                                                if db.response.clicked(){
                                                    if acc_mod.database == DatabaseSelection::Stable {
                                                        // self.save(Storage:)
                                                        set_db_selection(DatabaseSelection::Stable);
                                                    } else {
                                                        set_db_selection(DatabaseSelection::Beta);
                                                    }
                                                    info!("Database changed: {:?}", acc_mod.database);
                                                }
                                            });

                                            ui.add_space(5.0);
                                            
                                            TextEdit::singleline(&mut acc_mod.password)
                                                .hint_text("Password")
                                                .desired_width(180.0)
                                                .password(true)
                                                .ui(ui);

                                            ui.add_space(5.0);

                                            if Button::new("Update Account")
                                                .fill(Color32::from_rgb(30, 30, 35))
                                                .min_size(Vec2::new(140.0, 15.0))
                                                .ui(ui)
                                                .clicked()
                                            {
                                                let email = if usr.email.ends_with("@pclaptops.com"){
                                                    usr.email.clone()
                                                } else {
                                                    format!("{}@pclaptops.com", usr.email.clone())
                                                };

                                                let acc_mod = AccountMod {
                                                    email,
                                                    name: usr.name.clone(),
                                                    store: acc_mod.store,
                                                    ..Default::default()
                                                };

                                                info!("Account Mod: {:?}", acc_mod);
                                                acc_mod.mod_account(appstate_tx.clone(), usr.id.key().to_string().clone());
                                                match appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                                                    Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                                    Err(e) => error!("Error {e:?}"),
                                                }
                                            }
                                            ui.add_space(5.0);
                                            
                                            let button = Button::new("Change Password")
                                                .fill(Color32::from_rgb(30, 30, 35))
                                                .min_size(Vec2::new(140.0, 15.0));

                                            let enabled_button = ui.add_enabled(if acc_mod.password.len() > 0 { true } else { false }, button);

                                            if enabled_button.clicked() {
                                                acc_mod.change_password(usr.id.key().to_string().clone());
                                                match appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                                                    Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                                    Err(e) => error!("Error {e:?}"),
                                                }
                                            }
                                        }
                                    });
                                    ui.add_space(100.0);
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
