use std::collections::HashMap;

use crate::{
    app_state::SharedContext, tabs::ai_playground::ChatThread, PlatformSpawner, Spawner
};
use database::{
    live_data::listen_data,
    schema::{
        utilities::{get_store_users, get_tasks_for_store},
        NOTIFICATION_TABLE, TASK_NOTE_TABLE, TASK_TABLE,
    },
};
use crate::ui_tools::{theme_config::ThemeConfig, toasts::{Toast, ToastKind, ToastOptions}};
use log::info;

impl SharedContext {
    pub fn load_data(&mut self) -> bool {
        // get all of our channel Senders from crossbeam to get user/store/completed tasks,
        // as well as store users and live task notifications
        let live_tasks_tx = self.live_tasks_tx.clone();
        let notes_tx = self.notes_tx.clone();
        let live_notif_tx = self.live_notification_tx.clone();        

        if let Some(usr) = self.current_user.as_ref() {
            self.store_selection = std::convert::Into::<u64>::into(usr.store);

            info!("Getting Initial data: {}", self.store_selection);
            let user = usr.clone();
            let name = usr.name.clone();

            if self.tasks.is_empty() || self.store_users.is_empty() {
                let initial_tasks_tx = self.initial_tasks_tx.clone();
                let store_users_tx = self.store_users_tx.clone();
                let store = usr.store.as_str().to_string().clone();

                PlatformSpawner::spawn(async move {
                    let get_store_users = get_store_users(store_users_tx, user.clone().store).await;
                    info!("get_store_users: {get_store_users:?}");
                });

                PlatformSpawner::spawn(async move {
                    let get_tasks = get_tasks_for_store(initial_tasks_tx, store).await;
                    info!("get_tasks: {get_tasks:?}");
                });
            }

            PlatformSpawner::spawn(async move {
                let listen_data = listen_data(notes_tx, TASK_NOTE_TABLE).await;
                info!("listen_task_notes: {listen_data:?}");
            });

            PlatformSpawner::spawn(async move {
                let listen_data = listen_data(live_tasks_tx, TASK_TABLE).await;
                info!("listen_tasks: {listen_data:?}");
            });

            PlatformSpawner::spawn(async move {
                let listen_data = listen_data(live_notif_tx.clone(), NOTIFICATION_TABLE).await;
                info!("listen_notifications: {listen_data:?}");
            });

            match serde_json::from_value::<ThemeConfig>(usr.user_settings.color_scheme.clone()) {
                Ok(color_settings) => self.theme_config = color_settings.clone(),
                Err(e) => info!("Error setting theme config: {e:?}"),
            }

            let toast = &mut self.toasts;
            let auth_toast = Toast {
                kind: ToastKind::Success,
                text: format!("Logged in successfully\nWelcome, {}", name).into(),
                options: ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(6.0),
            };
            toast.add(auth_toast);
            true
        } else {
            info!("4");
            false
        }
    }

    pub fn receive(&mut self) {
        if let Ok(mut tasks) = self.initial_tasks_rx.try_recv() {
            log::info!("Got new tasks: {:?}", &tasks.len());
        
            // Indicate that filtering needs to be rerun
            self.rerun_filtering_store_tasks = true;
            self.rerun_filtering_completed = true;
        
            // Clear layout-related data for specific pages
            for (page, layout) in self.task_layouts.iter_mut() {
                if page == "CompletedTasks" || page == "StoreTasks" {
                    layout.task_map.clear();
                    layout.assignees.clear();
                    layout.search_inputs.clear();
                }
            }
        
            // Filter and append new tasks
            let existing_tasks = &mut self.tasks;
            for new_task in tasks.drain(..) {
                // Avoid duplicates by checking if the new task already exists
                if !existing_tasks.iter().any(|task| task == &new_task) {
                    existing_tasks.push(new_task);
                }
            }
        }
        

        if let Ok(users) = self.store_users_rx.try_recv() {
            for (page, layout) in self.task_layouts.iter_mut() {
                match page.as_str() {  
                    "CompletedTasks" | "StoreTasks" => {
                        layout.task_map.clear();
                        layout.assignees.clear();
                        layout.search_inputs.clear();
                    }
                    _ => {}
                }
                layout.update_assignees(users.clone());
            }
            // log::info!("Got new users: {:?}", users);
            self.rerun_filtering_store_tasks = true;
            self.rerun_filtering_completed = true;
            self.store_users.clear();
            self.store_users = users;
        }

        if let Ok(thread_obj) = self.ai_thread_channel.1.try_recv() {
            let mut thread_map = HashMap::new();
            self.ai_playground.save_chats = true;
            thread_map.insert(thread_obj.id.clone(), ChatThread {
                id: thread_obj.id.clone(),
                messages: Vec::new(),
                images: Vec::new(),
                input: String::new(),
            });
            self.ai_playground.selected_thread = thread_obj.id;
            self.ai_playground.set_threads(thread_map);
        }
    }
}
