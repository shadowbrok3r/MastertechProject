pub mod aging_tasks;
pub mod ai_playground;
pub mod completed_tasks;
pub mod customer;
pub mod github_issue;
pub mod json_viewer;
pub mod logger;
pub mod my_tasks;
pub mod query_builder;
pub mod quote_fulfilled_tasks;
pub mod stock;
pub mod store_tasks;
pub mod task_audit;
pub mod terminal;
pub mod toolbox;
pub mod web_console;

use super::app_state::MtechServerContext;
use eframe::egui::{Ui, WidgetText};
use egui_dock::{NodeIndex, SurfaceIndex, TabViewer};
use logger::logger_ui;

impl MtechServerContext {
    pub fn simple_demo_menu(&mut self, ui: &mut Ui) {
        if ui.button("Open...").clicked() {
            ui.close_menu();
        }
        ui.menu_button("SubMenu", |ui| {
            ui.menu_button("SubMenu", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_menu();
                }
                let _ = ui.button("Item");
            });
            ui.menu_button("SubMenu", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_menu();
                }
                let _ = ui.button("Item");
            });
            let _ = ui.button("Item");
            if ui.button("Open...").clicked() {
                ui.close_menu();
            }
        });
        ui.menu_button("SubMenu", |ui| {
            let _ = ui.button("Item1");
            let _ = ui.button("Item2");
            let _ = ui.button("Item3");
            let _ = ui.button("Item4");
            if ui.button("Open...").clicked() {
                ui.close_menu();
            }
        });
        let _ = ui.button("Very long text for this item");
    }
}

impl TabViewer for MtechServerContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab.as_str() {
            "Lil menu" => self.simple_demo_menu(ui),
            "Terminal" => self.terminal(ui),
            "My Tools" => self.toolbox(ui),
            "Store Tasks" => self.store_tasks(ui),
            "My Tasks" => self.my_tasks(ui),
            "Ai Playground" => self.ai_playground(ui),
            "Web Console" => self.web_console(ui),
            "Completed Tasks" => self.completed_tasks(ui),
            "Bug Report" => self.github(ui),
            "Customers" => self.customer_view(ui),
            "Logs" => logger_ui().show(ui),
            "Query Builder" => self.query_builder(ui),
            "Json Viewer" => self.json_viewer(ui),
            "Stock" => self.stock_viewer(ui),
            "Task Audit" => self.task_table_viewer(ui),
            _ => {}
        }
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        tab: &mut Self::Tab,
        _surface_index: SurfaceIndex,
        _node_index: NodeIndex,
    ) {
        match tab.as_str() {
            "My Tasks" => self.simple_demo_menu(ui),
            _ => {
                ui.label(tab.to_string());
                ui.label("This is a context menu");
            }
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.as_str().into()
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        self.open_tabs.remove(tab);
        true
    }

    fn on_add(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
        self.added_nodes.push((surface_index, node_index));
    }

    fn add_popup(&mut self, ui: &mut Ui, _surface_index: SurfaceIndex, _node_index: NodeIndex) {
        ui.set_width(100.0);
        let tabs = &[
            &"Bug Report".to_string(),
            &"Terminal".to_string(),
            &"My Tools".to_string(),
            &"Web Console".to_string(),
            &"Store Tasks".to_string(),
            &"My Tasks".to_string(),
            &"Ai Playground".to_string(),
            &"Completed Tasks".to_string(),
            &"Customers".to_string(),
            &"Logs".to_string(),
            &"Json Viewer".to_string(),
            &"Query Builder".to_string(),
            &"Stock".to_string(),
            &"Task Audit".to_string(),
        ];

        for tab in tabs {
            if ui
                .selectable_label(self.open_tabs.contains(*tab), *tab)
                .clicked()
            {
                if !self.open_tabs.contains(*tab) {
                    self.on_add(SurfaceIndex::main(), NodeIndex::root());
                }
            }
        }
    }
}
