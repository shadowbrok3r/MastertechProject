use displays::app_state::{AppState, MainPages};
use crate::app_state::MtechServer;
use eframe::egui;
use log::info;


impl eframe::App for MtechServer {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        displays::ui_tools::font_atlas_watch::watch(ctx);
        self.receive(frame, ctx);

        if let AppState::NoAuth(reason) = &self.shared_ctx.state {
            if reason.contains("Already connected") {
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
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(2).unwrap();
        });

        self.shared_ctx.menu_bar(ui);

        match &self.shared_ctx.state {
            AppState::Authenticated(MainPages::Downloads) => self.shared_ctx.downloads_page(ui),
            AppState::Authenticated(MainPages::UserPreferences) => self.shared_ctx.account_settings_page(ui, self.shared_ctx.app_state_tx.clone()),
            AppState::Authenticated(_) => self.shared_ctx.main_page(ui),
            AppState::CreateAccount => self.shared_ctx.signup_page(
                ui,
                self.shared_ctx.db_tx.clone(),
                self.shared_ctx.app_state_tx.clone(),
            ),
            AppState::NoAuth(reason) => {
                if !reason.contains("Already connected") {
                    self.shared_ctx.login_page(
                        ui,
                        self.shared_ctx.db_tx.clone(),
                        self.shared_ctx.app_state_tx.clone(),
                    )
                }
            }
        }
    }

    fn persist_egui_memory(&self) -> bool { true }
}
