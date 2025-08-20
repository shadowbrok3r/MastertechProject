use database::schema::{utilities::NotificationMod, Notification};
use database::live_data::{handle_live_delete, update_or_insert_anything};
use eframe::egui::*;

use crate::{PlatformSpawner, Spawner};

#[derive(Default, serde::Serialize)]
pub struct NotificationCenter {
    pub notifications: Vec<Notification>,
    pub read_notifications: bool,
    pub show_notifications: bool,
    pub search_query: String,
}

impl NotificationCenter {
    pub fn ui(
        &mut self, 
        ui: &mut Ui,
        task_names: &std::collections::BTreeSet<String>, 
        ui_actions_tx: crossbeam::channel::Sender<crate::TaskUiActions>, 
        tasks: &Vec<database::schema::LiveTaskPayload>
    ) {
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            if self.notifications.is_empty() {
                if ui.button(RichText::new("Show Notifications").heading()).clicked() {
                    self.show_notifications = true;
                }
            }
        });

        // Search input under the Show Notifications button
        ui.vertical_centered(|ui| {
            let edit = TextEdit::singleline(&mut self.search_query)
                .hint_text("Search notifications...")
                .desired_width(ui.available_width() - 120.0);
            ui.add(edit);
            if !self.search_query.is_empty() {
                if ui.button("Clear").clicked() {
                    self.search_query.clear();
                }
            }
        });

        // Build filtered index list so mutations apply to the real data
        let query = self.search_query.trim().to_lowercase();
        let filtered_indices: Vec<usize> = self
            .notifications
            .iter()
            .enumerate()
            .filter_map(|(idx, n)| {
                let matches_query = if query.is_empty() {
                    true
                } else {
                    n.notification_description.to_lowercase().contains(&query)
                        || n.notification_type.to_lowercase().contains(&query)
                };
                if !matches_query {
                    return None;
                }
                if self.read_notifications {
                    (n.status == "Read").then_some(idx)
                } else {
                    (n.status == "Unread").then_some(idx)
                }
            })
            .collect();

        ui.horizontal(|ui| {
            // Left: Read button
            let read_button = Button::new(RichText::new("Read").color(Color32::from_rgba_premultiplied(42, 222, 192, 60)))
                .stroke(ui.style().visuals.noninteractive().fg_stroke)
                .fill(ui.style().visuals.noninteractive().bg_fill);
            if ui.add(read_button).clicked() {
                self.read_notifications = true;
            }

            ui.add_space(ui.available_width()/3.0);

            // Middle: Mark All toggle for the currently filtered set
            let all_label = if self.read_notifications { "Mark All Unread" } else { "Mark All Read" };
            if ui
                .add(
                    Button::new(RichText::new(all_label))
                        .stroke(ui.style().visuals.noninteractive().fg_stroke)
                        .fill(ui.style().visuals.noninteractive().bg_fill),
                )
                .on_hover_text("Apply to all currently filtered notifications")
                .clicked()
            {
                let make_read = !self.read_notifications; // if viewing Unread -> mark all Read; if viewing Read -> mark all Unread
                self.mark_all_by_indices(&filtered_indices, make_read);
            }

            // Right: Unread button (push to right)
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let unread_button = Button::new(RichText::new("Unread").color(Color32::from_rgb(191, 33, 101)))
                    .stroke(ui.style().visuals.noninteractive().fg_stroke)
                    .fill(ui.style().visuals.noninteractive().bg_fill);
                if ui.add(unread_button).clicked() {
                    self.read_notifications = false;
                }
            });
        });

    let row_height = 100.;
        let total_rows = filtered_indices.len();
        let scroll_area = ScrollArea::vertical().auto_shrink(false);
        ui.ctx().options_mut(|o| o.input_options.line_scroll_speed = 15.0);

        ui.scope(|ui| {
            let _ = ui.style_mut().visuals.extreme_bg_color + Color32::from_rgb(30,30,30);
            // clone indices for the closure
            let indices = filtered_indices.clone();
            scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
                for row in row_range {
                    if let Some(&idx) = indices.get(row) {
                        let notification = &mut self.notifications[idx];
                        
                        Frame::new()
                        .corner_radius(eframe::egui::CornerRadius::same(8))
                        .fill(ui.style().visuals.code_bg_color)
                        .inner_margin(Margin::same(6))
                        .outer_margin(Margin::same(3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    Color32::from_rgba_premultiplied(42, 222, 192, 60),
                                    RichText::new(notification.notification_type.clone())
                                );
                                
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    // Dynamic toggle label + color for clarity
                                    let (toggle_label, toggle_color) = if notification.status == "Read" {
                                        ("Mark Unread", Color32::from_rgb(191, 33, 101))
                                    } else {
                                        ("Mark Read", Color32::from_rgba_premultiplied(42, 222, 192, 60))
                                    };
                                    
                                    let button = Button::new(
                                    RichText::new(toggle_label).color(toggle_color)
                                    )
                                    .stroke(ui.style().visuals.noninteractive().fg_stroke)
                                    .fill(ui.style().visuals.noninteractive().bg_fill)
                                    .min_size(Vec2::new(80.0, 22.0))
                                    .ui(ui)
                                    .on_hover_text("Toggle this notification's status");
                                    
                                    if button.clicked() {
                                        // Toggle read/unread locally and persist
                                        let mut notif = notification.clone();
                                        if notification.status == "Read" {
                                            notification.status = "Unread".to_string();
                                            PlatformSpawner::spawn(async move {
                                                let _ = notif.mark_notification(false).await;
                                            });
                                        } else {
                                            notification.status = "Read".to_string();
                                            PlatformSpawner::spawn(async move {
                                                let _ = notif.mark_notification(true).await;
                                            });
                                        }
                                    }
                                });
                            });
                            
                            ui.separator();
                            crate::ui_tools::show_notification(
                                ui,
                                &notification.notification_description,
                                &task_names,
                                ui_actions_tx.clone(),
                                &tasks,
                            );
                        })
                        .inner;
                    }
                }
            });
        });
    }

    pub fn set_notifications(&mut self, notifications: Vec<Notification>) {
        self.notifications = notifications;
    }

    // Apply live updates from SurrealDB to the center's own list
    pub fn apply_update(&mut self, notification: Notification) {
        let _ = update_or_insert_anything(&mut self.notifications, notification);
    }

    pub fn apply_delete(&mut self, notification: Notification) {
        let _ = handle_live_delete(&mut self.notifications, notification);
    }

    // Bulk mark all currently filtered indices as read/unread and persist
    fn mark_all_by_indices(&mut self, indices: &[usize], make_read: bool) {
        for &idx in indices {
            if let Some(n) = self.notifications.get_mut(idx) {
                let should_change = (make_read && n.status != "Read") || (!make_read && n.status == "Read");
                if should_change {
                    let mut clone = n.clone();
                    n.status = if make_read { "Read".to_string() } else { "Unread".to_string() };
                    PlatformSpawner::spawn(async move {
                        let _ = clone.mark_notification(make_read).await;
                    });
                }
            }
        }
    }
}