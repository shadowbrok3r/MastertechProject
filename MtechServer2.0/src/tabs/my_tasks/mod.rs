use crate::{app_state::MtechServerContext, utilities::{displays::tasks::task_layout::TaskLayout, FilterTasks}};
use std::collections::BTreeMap;
use database::schema::Status;
use eframe::egui::Ui;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        if let Some(users) = self.store_users.as_ref(){
            let page = "MyTasks";
            let current_user = self.current_user.as_ref().unwrap();

            let mut vals = Status::VALUES;
                // Define the custom sort order
            let order = |name: Status| match name.as_str() {
                "Todo" => 1,
                "In Repair" => 2,
                "Complete" => 3,
                _ => 4, // Default case if there are other unexpected items
            };
            
            vals.sort_unstable_by_key(|x| order(x.clone()));
            
            if let Some(layout) = self.task_layouts.get_mut(page){
                if self.rerun_filtering_my_tasks{
                    self.rerun_filtering_my_tasks = false;
                    let mut map = BTreeMap::new();
                    vals.iter_mut().for_each(|status| {
                        let filtered = self.tasks.filter_by_status(&status).filter_by_assignee(current_user); //.filter_by_my_store(users, current_user);
                        map.entry(status.as_str().to_string()).or_insert(filtered);
                    });
                    layout.task_map = map;
                }
                layout.layout_cols(ui);
            } else {
                let mut map = BTreeMap::new();
                vals.iter_mut().for_each(|status| {
                    let filtered = self.tasks.filter_by_status(&status).filter_by_assignee(current_user); //.filter_by_my_store(users, current_user);
                    map.entry(status.as_str().to_string()).or_insert(filtered);
                });
                let col_names = vals.iter().map(|v| v.as_str().to_string()).collect::<Vec<String>>();
                // let user_names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
                let layout = TaskLayout::new(map, col_names, self.ui_actions_tx.clone(), users.clone());
                self.task_layouts.insert(page.to_string(), layout);
            }
        }
    }
}