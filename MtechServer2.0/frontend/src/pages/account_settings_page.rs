use eframe::egui::{Align, Button, CentralPanel, Color32, ComboBox, Context, Direction, FontId, Frame, Layout, RichText, TextEdit, Vec2, Widget};
use crate::app_state::{AppState, MainPages, MtechServer};
use database::{schema::{Store, USER_TABLE}, DATABASE};
use surrealdb::{opt::RecordId, sql::Id};
use egui_extras::{Size, StripBuilder};
use wasm_bindgen_futures::spawn_local;
use crossbeam::channel::Sender;
use serde::Serialize;
use log::info;

#[derive(Serialize, Debug, Default, Clone)]
pub struct AccountMod {
    pub name: String,
    pub email: String,
    pub password: String,
    pub store: Store
}

impl AccountMod{
    pub fn mod_account(&self, _appstate_tx: Sender<AppState>, user_id: Id){
        let acc_mod: AccountMod = Self {
            name: self.name.clone(),
            email: self.email.clone(),
            password: self.password.clone(),
            store: self.store.clone()
        };
        let account_modification = acc_mod.clone();
        spawn_local(async move {
            let x: Result<Option<RecordId>, surrealdb::Error> = DATABASE.update((USER_TABLE, user_id))
                .merge(account_modification).await;
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
                            .sizes(Size::remainder(), 3)
                            .vertical(|mut s| 
                        {
                            s.cell(|ui| 
                            {
                                ui.group(|ui| 
                                { 
                                    ui.add_space(100.0);

                                    ui.label(RichText::new("Modify Account").heading());
                                    let font = FontId::proportional(18.0);
                                    ui.style_mut().override_font_id = Some(font);
    
                                    ui.add_space(20.0);

                                    if let (Some(ref mut usr), Some(acc_mod)) = (self.context.current_user.clone(), self.account_mut()){
                                        let width = ui.available_width() / 5.9;

                                        ui.horizontal_top(|ui| {
                                            ui.add_space(width);
                                            TextEdit::singleline(&mut usr.name)
                                                .hint_text("Name")
                                                .desired_width(180.0)
                                                .ui(ui);

                                            ui.add_space(5.0);

                                            TextEdit::singleline(&mut usr.email)
                                                .hint_text("Email")
                                                .desired_width(180.0)
                                                .ui(ui);

                                            // let mut email = usr.email.split_once("@").unwrap_or(("", "")).0;
                                            // let text_edit = TextEdit::singleline(&mut email).desired_width(180.0);
                                        
                                            // let output = text_edit.show(ui);
                                            // let chars = usr.email.chars().count() as f32;
                                            // let painter = ui.painter_at(output.response.rect);
                                            // let text_color = Color32::from_rgba_premultiplied(100, 100, 100, 100);
                                            // let font = FontId::proportional(18.0);
                                            // let galley = painter.layout(
                                            //     String::from("@pclaptops.com"),
                                            //     font,
                                            //     text_color,
                                            //     f32::INFINITY
                                            // );
                                            // painter.galley(Pos2::new(output.galley_pos.x + (chars as f32 * 11.75), output.galley_pos.y), galley, text_color);
                                        });

                                        ui.add_space(5.0);

                                        ui.horizontal_top(|ui| {
                                            ui.add_space(width);
                                            TextEdit::singleline(&mut acc_mod.password)
                                                .hint_text("Password")
                                                .desired_width(180.0)
                                                .password(true)
                                                .ui(ui);

                                            ui.add_space(5.0);

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

                                        ui.add_space(5.0);
                                                
                                        ui.add_space(10.0);

                                        ui.vertical_centered(|ui| {
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
                                                    password: acc_mod.password.clone(),
                                                };

                                                info!("Account Mod: {:?}", acc_mod);
                                                acc_mod.mod_account(appstate_tx.clone(), usr.id.0.id.clone());
                                                match appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                                                    Ok(_) => info!("Sent appstate"), // drop(appstate_tx)
                                                    Err(e) => info!("Error {e:?}"),
                                                }
                                            }
                                        });
                                    }
                                    ui.add_space(100.0);
                                });
                            });
                            s.empty();
                            s.empty();
                        });
                    });
                    s.empty();
                });
        });
    }
}