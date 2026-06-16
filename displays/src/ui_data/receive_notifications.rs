use std::collections::BTreeSet;

use crate::{app_state::SharedContext, ui_tools::{theme, toasts::ToastStyle}};
use database::schema::LiveTaskPayload;
use crate::{ui_tools::toasts::{Toast, ToastKind, ToastOptions}, TaskUiActions};
use eframe::egui::{Button, FontId, Margin, RichText, Ui, Widget};
use log::info;
use regex::Regex;
use database::live_data::Action;

impl SharedContext {
    pub fn receive_notification(&mut self, ctx: &eframe::egui::Context) {
        if let Ok((action, notification)) = self.live_notification_rx.try_recv() {
            // Live-query canary: confirm the notification stream is alive and
            // never surface it as a real notification.
            if notification.notification_type == "live_query_check" {
                if matches!(action, Action::Create | Action::Update)
                    && self.canary_nonce.as_deref()
                        == Some(notification.notification_description.as_str())
                {
                    self.canary_nonce = None;
                    self.canary_sent_at = None;
                    self.reconnect_attempts = 0;
                    self.needs_reconnect = false;
                    self.show_reload_prompt = false;
                }
                return;
            }

            // Test text
            let mut inputs = BTreeSet::new();
            for task in self.tasks.iter() {
                inputs.insert(task.task_name.clone());
            }

            let user = self.current_user.clone().unwrap_or_default();
            if notification.user == user.get_id() {
                info!("Action: {action:?} - Notification: {notification:?}");
            }
            match action {
                Action::Create => {
                    if notification.notification_type.as_str() == "Admin" {
                        self.notification_modal = Some(notification.clone());
                    } else {
                        let username_regex = Regex::new(r"tagged (\w+\.\w+)").unwrap();
                        let task_name_regex = Regex::new(r"in task (.+)").unwrap();

                        if let Some(usr) = self.current_user.as_ref() {
                            // Find the username
                            if let Some(captures) =
                                username_regex.captures(&notification.notification_description)
                            {
                                let username = captures.get(1).unwrap().as_str();
                                info!("Found username: {}", username);
                            } else {
                                info!("Username not found");
                            }
                            // Find the task name
                            if let Some(captures) =
                                task_name_regex.captures(&notification.notification_description)
                            {
                                let task_name = captures.get(1).unwrap().as_str();
                                // Check if the task name exists in the inputs BTreeSet
                                if inputs.contains(task_name) {
                                    info!("Found task name: {}", task_name);
                                } else {
                                    info!("Task name not found in inputs");
                                }
                            } else {
                                info!("Task name not found");
                            }
                            if notification.user == usr.get_id() {
                                self.notification_center.read_notifications = false;
                                let toast = &mut self.toasts;
                                let auth_toast = Toast {
                                    kind: ToastKind::Info,
                                    text: RichText::new(format!(
                                        "Notification\n\n{}",
                                        notification.notification_description
                                    ))
                                    .color(theme::success_ctx(ctx))
                                    .font(FontId::monospace(15.))
                                    .into(),
                                    options: ToastOptions::default().duration(None),
                                    style: ToastStyle::default(),
                                };
                                toast.add(auth_toast);
                            }
                        }
                        // Keep NotificationCenter as the single source of truth
                        self.notification_center.apply_update(notification.clone());
                    }
                }
                Action::Update => {
                    self.notification_center.apply_update(notification.clone());
                }
                Action::Delete => {
                    self.notification_center.apply_delete(notification.clone());
                }
            };
        }

        if let Ok(notifications) = self.notification_rx.try_recv() {
            let notifications: Vec<_> = notifications
                .into_iter()
                .filter(|n| n.notification_type != "live_query_check")
                .collect();
            self.notification_center.set_notifications(notifications);
        }
    }
}

pub fn find_task_in_description(
    notification_description: &str,
    task_names: &BTreeSet<String>, // BTreeSet of task names
) -> Vec<String> {
    // Regex to capture the task name after "in task "
    let task_name_regex = Regex::new(r"in task (.+)").unwrap();

    // Use regex to find the task name in the description
    let matches: Vec<String> = task_name_regex
        .captures(notification_description)
        .and_then(|caps| caps.get(1)) // Get the first capture group (task name)
        .map(|match_task_name| {
            let task_name = match_task_name.as_str().to_string();

            // Check if the extracted task name is in the set of task names
            if task_names.contains(&task_name) {
                Some(task_name) // Return the matching task name
            } else {
                None
            }
        })
        .flatten() // Unwrap the optional match
        .into_iter()
        .collect();

    matches
}

pub fn show_notification(
    ui: &mut Ui,
    notification_description: &str,
    task_names: &BTreeSet<String>,
    ui_actions_tx: crossbeam::channel::Sender<TaskUiActions>,
    tasks: &Vec<LiveTaskPayload>,
) {
    // Find task names in the notification description using regex
    let matches = find_task_in_description(notification_description, task_names);

    // We assume only one match for simplicity; handle multiple matches if necessary
    if let Some(task_name) = matches.get(0) {
        // Find where the task name is in the notification description
        if let Some(pos) = notification_description.find(task_name) {
            // Split the text into before, task name, and after
            let before = &notification_description[..pos];
            let after = &notification_description[pos + task_name.len()..];

            // Display the text parts with different formatting
            eframe::egui::Frame::new()
                .fill(ui.style().visuals.window_fill)
                .corner_radius(eframe::egui::CornerRadius::same(12))
                .inner_margin(Margin::same(15))
                .outer_margin(Margin::same(5))
                .show(ui, |ui| {
                    info!("{pos:?}, {before:?}, {task_name:?}, {after:?}");
                    ui.horizontal_wrapped(|ui| {
                        // Show the text before the task name
                        ui.label(RichText::new(before));

                        // Show the task name in a different color (e.g., blue)
                        if Button::new(
                            RichText::new(task_name)
                                .color(theme::accent(ui)),
                        )
                        .ui(ui)
                        .clicked()
                        {
                            let task = tasks.iter().find(|&x| {
                                x.task_name == *task_name
                                    || format!("{}", x.service_number.clone().unwrap_or_default())
                                        == format!("{}", *task_name)
                            });

                            if let Some(task) = task {
                                let _ = ui_actions_tx
                                    .try_send(TaskUiActions::OpenTaskModal(task.clone()));
                            }
                        }

                        // Show the text after the task name
                        ui.label(after);
                    });
                });
        } else {
            // If no task name is found, display the whole description normally
            ui.label(notification_description);
        }
    } else {
        // If no task name is matched, just show the description
        ui.label(notification_description);
    }
}
