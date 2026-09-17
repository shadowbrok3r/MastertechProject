use displays::app_state::default_tree;
use displays::tabs::admin_console::RightPanel;
use displays::tabs::{DockSession, TabContext, TabId, WorkMode};
use displays::ui_tools::{framed_controls::selectable_card, glass_card, icons, info_card, theme};
use eframe::egui::{self, RichText, Ui};

use crate::app_state::MasterTechApp;

impl MasterTechApp {
    /// Swaps the dock to `next`. Nothing on the context is touched, so a running stress
    /// test, the scripts queue and unsaved TUR fields all survive the switch.
    pub fn apply_work_mode(&mut self, next: WorkMode, ctx: &egui::Context) {
        if self.context.work_mode.active == Some(WorkMode::Full) {
            self.context.work_mode.full_snapshot = Some(self.dock.tree.clone());
        }

        self.dock = match next {
            WorkMode::Full => {
                let mut session = match self.context.work_mode.full_snapshot.take() {
                    Some(tree) => DockSession { tree },
                    None => self.saved_layout(),
                };
                let visible = TabId::visible_for(TabContext::MastertechNative);
                session.tree.retain_tabs(|tab| visible.contains(tab));
                session
            }
            preset => preset.canonical_session(),
        };

        self.context.work_mode.active = Some(next);

        if next == WorkMode::Admin {
            self.context
                .shared_ctx
                .web_console_layout
                .set_right_panel(Some(RightPanel::Chat));
        }

        // Queued opens may name tabs the new mode does not have.
        self.context.pending_tab_opens.clear();
        self.context.pending_tab_adds.clear();
        self.context.pending_tab_removes.clear();
        self.context.pending_activate_tab = None;
        self.context.added_nodes.clear();

        ctx.request_repaint();
    }

    /// The operator's stored Full-mode layout, falling back through the legacy format.
    pub fn saved_layout(&self) -> DockSession {
        let Some(user) = self.context.shared_ctx.current_user.as_ref() else {
            return default_tree();
        };
        let layout = user.get_user_settings().get_ui_layout_mastertech();
        if let Ok(tree) = serde_json::from_value::<egui_dock::DockState<TabId>>(layout.clone()) {
            return DockSession { tree };
        }
        match serde_json::from_value::<egui_dock::DockState<String>>(layout) {
            Ok(legacy) => DockSession::from_legacy_tree(legacy),
            Err(e) => {
                log::error!("Could not get UI layout from user: {e:?}");
                default_tree()
            }
        }
    }

    pub fn work_mode_picker(&mut self, ui: &mut Ui) {
        let is_root = displays::tabs::admin_console::current_user_is_root();
        let mut chosen = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.label(
                    RichText::new("What are you working on?")
                        .heading()
                        .color(theme::strong_text(ui)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Pick a job and MasterTech opens just the tools for it.")
                        .color(theme::weak_text(ui)),
                );
                ui.add_space(20.0);
            });

            let card_width = ui.available_width().min(620.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(card_width);
                glass_card::titled_card(ui, icons::HOME, "Start a session", None, |ui| {
                    for mode in WorkMode::ALL {
                        ui.add_space(4.0);
                        let row = selectable_card(ui, mode.slug(), false, |ui| {
                            mode_row(ui, *mode, is_root);
                        });
                        if row.response.clicked() {
                            chosen = Some(*mode);
                        }
                    }
                    ui.add_space(8.0);
                    glass_card::hairline(ui);
                    ui.checkbox(
                        &mut self.context.work_mode.remember,
                        "Open this every time, and skip this screen",
                    );
                });
            });
        });

        if let Some(mode) = chosen {
            if self.context.work_mode.remember {
                self.save_default_work_mode(Some(mode));
            }
            let ctx = ui.ctx().clone();
            self.apply_work_mode(mode, &ctx);
        }
    }

    /// Stores the startup mode on the operator's account. `None` restores the picker.
    pub fn save_default_work_mode(&mut self, mode: Option<WorkMode>) {
        let slug = mode.map(|m| m.slug().to_owned());
        let Some(user) = self.context.shared_ctx.current_user.as_mut() else {
            return;
        };
        user.set_default_work_mode_local(slug.clone());
        let mut user = user.clone();
        tokio::spawn(async move {
            if let Err(e) = user.save_default_work_mode(slug).await {
                log::error!("Could not save the default work mode: {e:?}");
            }
        });
    }
}

/// One picker row. Every widget here stays non-interactive so the card keeps the click.
fn mode_row(ui: &mut Ui, mode: WorkMode, is_root: bool) {
    ui.horizontal(|ui| {
        ui.label(icons::icon_colored(mode.glyph(), theme::accent(ui)).size(22.0));
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(mode.title()).strong());
            ui.label(
                RichText::new(mode.blurb())
                    .small()
                    .color(theme::weak_text(ui)),
            );
            ui.add_space(3.0);
            ui.horizontal_wrapped(|ui| match mode.curated_tabs() {
                Some(_) => {
                    for tab in mode.visible_tabs(TabContext::MastertechNative, is_root) {
                        info_card::badge(
                            ui,
                            tab.title(TabContext::MastertechNative),
                            theme::accent_secondary(ui),
                        );
                    }
                }
                None => {
                    info_card::badge(ui, "every tab, your saved layout", theme::weak_text(ui));
                }
            });
        });
    });
}
