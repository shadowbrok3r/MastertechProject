use displays::app_state::{AppState, MainPages};
use crate::app_state::MtechServer;
use eframe::egui::Context;
use log::info;


impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // Branch out all the different crossbeam channels to receive
        // in their own methods to clean up a lot of boilerplate code
        // as well as being able to find specific code a lot easier
        // self.receive() is the same thing but those crossbeam channels
        // being received have literally one line in them that i dont want to
        // justify creating a separate file / module for
        self.receive(frame, ctx);
        self.menu_bar(ctx);

        // Handle changes to state from various places, such as
        // hitting the login button, clicking the 'home page' button
        // (which is clicking Mtechserver in the top middle of the page),
        // if session cookie expires (gets checked in the first_run method),
        // if manually logged out, etc
        match &self.context.shared_ctx.state {
            AppState::Authenticated(MainPages::Downloads) => self.context.shared_ctx.downloads_page(ctx),
            AppState::Authenticated(MainPages::UserPreferences) => self.context.shared_ctx.account_settings_page(ctx, self.context.shared_ctx.app_state_tx.clone()),
            AppState::Authenticated(_) => self.main_page(ctx),
            AppState::CreateAccount => self.context.shared_ctx.signup_page(
                ctx,
                self.context.shared_ctx.db_tx.clone(),
                self.context.shared_ctx.app_state_tx.clone(),
            ),
            AppState::NoAuth(reason) => {
                if reason.to_string().contains("Already connected") {
                    info!("Already connected");
                    if self.context.shared_ctx.current_user.is_some() {
                        if !self.context.shared_ctx.load_data(ctx) {
                            self.context.shared_ctx.first_run = true;
                            self.first_run(frame);
                            log::error!("4");
                            self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                        }
                    } else {
                        self.context.shared_ctx.first_run = true;
                        self.first_run(frame)
                    }
                    self.context.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);
                } else {
                    self.context.shared_ctx.login_page(
                        ctx,
                        self.context.shared_ctx.db_tx.clone(),
                        self.context.shared_ctx.app_state_tx.clone(),
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
