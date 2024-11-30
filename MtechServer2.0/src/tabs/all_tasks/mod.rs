use crate::{
    app_state::MtechServerContext,
    utilities::{displays::tasks::task_layout::TaskLayout, FilterTasks},
};
use database::schema::Status;
use eframe::egui::Ui;
use std::collections::BTreeMap;

impl MtechServerContext {
    pub fn company_tasks(&mut self, ui: &mut Ui) {
        if !self.shared_ctx.store_users.is_empty() {
            let page = "Company Tasks";
            let mut vals = Status::VALUES;
            // let current_user = self.current_user.as_ref().unwrap();
            if let Some(layout) = self.task_layouts.get_mut(page) {
                if self.rerun_filtering_completed {
                    // self.rerun_filtering_completed = false;
                    let mut map = BTreeMap::new();

                    vals.iter_mut().for_each(|status| {
                        let filtered = self.tasks.filter_by_status(&status);
                        map.entry(status.as_str().to_string()).or_insert(filtered);
                    });
                    layout.task_map = map;
                }
                layout.layout_cols(ui);
            } else {
                let mut map = BTreeMap::new();
                vals.iter_mut().for_each(|status| {
                    let filtered = self.tasks.filter_by_status(&status);
                    map.entry(status.as_str().to_string()).or_insert(filtered);
                });
                let col_names = vals
                    .iter()
                    .map(|v| v.as_str().to_string())
                    .collect::<Vec<String>>();

                let layout =
                    TaskLayout::new(map, col_names, self.ui_actions_tx.clone(), users.clone());
                self.task_layouts.insert(page.to_string(), layout);
            }
        }
    }
}
