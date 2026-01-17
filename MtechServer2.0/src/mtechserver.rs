use displays::app_state::{AppState, MainPages};
use crate::app_state::MtechServer;
use eframe::egui::Context;
use log::info;


impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // Branch out all the different crossbeam channels to receive
        // in their own methods to clean up a lot of boilerplate code
        // as well as being able to find specific code a lot easier
        self.receive(frame, ctx);
        self.shared_ctx.menu_bar(ctx); // , &mut self.open_tabs, &mut self.tree

        // Handle changes to state from various places, such as
        // hitting the login button, clicking the 'home page' button
        // (which is clicking Mtechserver in the top middle of the page),
        // if session cookie expires (gets checked in the first_run method),
        // if manually logged out, etc
        match &self.shared_ctx.state {
            AppState::Authenticated(MainPages::Downloads) => self.shared_ctx.downloads_page(ctx),
            AppState::Authenticated(MainPages::UserPreferences) => self.shared_ctx.account_settings_page(ctx, self.shared_ctx.app_state_tx.clone()),
            AppState::Authenticated(_) => self.shared_ctx.main_page(ctx),
            AppState::CreateAccount => self.shared_ctx.signup_page(
                ctx,
                self.shared_ctx.db_tx.clone(),
                self.shared_ctx.app_state_tx.clone(),
            ),
            AppState::NoAuth(reason) => {
                if reason.to_string().contains("Already connected") {
                    info!("Already connected");
                    let usr = self.shared_ctx.current_user.clone();
                    if let Some(user) = usr {
                        self.shared_ctx.load_data(ctx, &user);
                        let _ = self.shared_ctx.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                    } else {
                        self.shared_ctx.first_run = true;
                        self.first_run(ctx, frame);
                        log::error!("1");
                        self.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                    }
                } else {
                    self.shared_ctx.login_page(
                        ctx,
                        self.shared_ctx.db_tx.clone(),
                        self.shared_ctx.app_state_tx.clone(),
                    )
                }
            }
        }
    }
}
