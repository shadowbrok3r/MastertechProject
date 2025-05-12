use eframe::egui::{Align, Button, CentralPanel, Color32, Context, Direction, FontId, Frame, Key, Layout, RichText, TextEdit, Vec2, Widget};
use database::{schema::Store, DATABASE};
use crate::app_state::{AppState, MainPages, MtechServer};
use egui_extras::{Size, StripBuilder};
use wasm_bindgen_futures::spawn_local;
use crossbeam::channel::Sender;
use serde::Serialize;
use log::{error, info};

#[derive(Serialize, Debug, Default, Clone)]
pub struct AccountMod {
    name: String,
    email: String,
    password: String,
    retyped_password: String,
    store: Store,
}

impl AccountMod{
    // pub fn mod_account(&self, _appstate_tx: Sender<AppState>, user_id: String){
    //     let acc_mod: AccountMod = Self {
    //         name: self.name.clone(),
    //         email: self.email.clone(),
    //         password: self.password.clone(),
    //         ..Default::default()
    //     };

    //     spawn_local(async move {
    //         let mod_user_result: Result<Response, surrealdb::Error> = DATABASE
    //             .query("fn::modify_account($user, $new)")
    //             .bind(("user", user_id))
    //             .bind(("new", acc_mod))
    //             .await;
    //         info!("mod_user_result: {mod_user_result:?}");
    //     });
    // }

    pub fn change_password(&self){
        let password = self.password.clone();
        spawn_local(async move {
            let x: Result<surrealdb::Response, surrealdb::Error> = DATABASE
                .query("UPDATE user SET password = crypto::argon2::generate($pass) WHERE id == $auth.id")
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
                                        ui.label(RichText::new("Update Password").heading());
                                        let font = FontId::proportional(15.0);
                                        ui.style_mut().override_font_id = Some(font);
        
                                        ui.add_space(20.0);

                                        if let (Some(ref mut usr), Some(acc_mod)) = (self.context.shared_ctx.current_user.clone(), self.account_mut()){
                                            // let user_pre_mod = usr.clone();
                                            // let width = ui.available_width() / 3.0 + 10.0;

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

                                            // ui.add_space(5.0);

                                            // ui.horizontal_top(|ui| {
                                            //     ui.add_space(width);
                                            //     ComboBox::new("StoreComboBox", "")
                                            //         .selected_text(acc_mod.store.as_str())
                                            //         .width(230.)
                                            //         .show_ui(ui, |ui| 
                                            //     {
                                            //         for store in Store::VALUES {
                                            //             ui.selectable_value(&mut acc_mod.store, store, store.as_str());
                                            //         }
                                            //     });
                                            // });

                                            // ui.horizontal_top(|ui| {
                                            //     ui.add_space(width);
                                            //     let db = ComboBox::new("Database", "")
                                            //         .selected_text(format!("{:?}", acc_mod.database))
                                            //         .width(230.)
                                            //         .show_ui(ui, |ui| 
                                            //     {
                                            //         ui.selectable_value(&mut acc_mod.database, DatabaseSelection::Stable, "Stable");
                                            //         ui.selectable_value(&mut acc_mod.database, DatabaseSelection::Beta, "Beta");
                                            //     });
                                            //     if db.response.clicked(){
                                            //         if acc_mod.database == DatabaseSelection::Stable {
                                            //             // self.save(Storage:)
                                            //             set_db_selection(DatabaseSelection::Stable);
                                            //         } else {
                                            //             set_db_selection(DatabaseSelection::Beta);
                                            //         }
                                            //         info!("Database changed: {:?}", acc_mod.database);
                                            //     }
                                            // });

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

                                            // ui.add_space(5.0);

                                            // if Button::new("Update Account")
                                            //     .fill(Color32::from_rgb(30, 30, 35))
                                            //     .min_size(Vec2::new(140.0, 15.0))
                                            //     .ui(ui)
                                            //     .clicked()
                                            // {
                                            //     let email = if usr.email.ends_with("@pclaptops.com"){
                                            //         usr.email.clone()
                                            //     } else {
                                            //         format!("{}@pclaptops.com", usr.email.clone())
                                            //     };

                                            //     let acc_mod = AccountMod {
                                            //         email,
                                            //         name: usr.name.clone(),
                                            //         store: acc_mod.store,
                                            //         ..Default::default()
                                            //     };

                                            //     info!("Account Mod: {:?}", acc_mod);
                                            //     acc_mod.mod_account(appstate_tx.clone(), usr.id.key().to_string().clone());
                                            //     match appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                                            //         Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                            //         Err(e) => error!("Error {e:?}"),
                                            //     }
                                            // }
                                            ui.add_space(5.0);

                                            let change_pw_txt = if password_check {" Change Password "} else { " Passwords must match " };
                                            
                                            let button = Button::new(change_pw_txt)
                                                .fill(Color32::from_rgb(30, 30, 35))
                                                .min_size(Vec2::new(140.0, 15.0));

                                            let enabled_button = ui.add_enabled(acc_mod.password.len() > 3 && password_check, button);
                                            let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

                                            if enabled_button.clicked() || accepted_by_keyboard {
                                                acc_mod.change_password();
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
