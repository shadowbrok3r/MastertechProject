use database::schema::{utilities::NotificationMod, Notification};
use database::live_data::{handle_live_delete, update_or_insert_anything};
use eframe::egui::*;

use crate::{PlatformSpawner, Spawner};

/// Known notification categories
pub const NOTIFICATION_CATEGORIES: &[&str] = &[
    "All",
    "Task Update",
    "Task Created", 
    "ALERT",
    "Admin",
    "System",
];

#[derive(Default, serde::Serialize)]
pub struct NotificationCenter {
    pub notifications: Vec<Notification>,
    pub read_notifications: bool,
    pub show_notifications: bool,
    pub search_query: String,
    /// Selected category filter (None = "All")
    #[serde(skip)]
    pub selected_category: Option<String>,
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

        // Category filter buttons
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            // "All" button
            let all_selected = self.selected_category.is_none();
            let all_color = if all_selected {
                Color32::from_rgb(42, 222, 192)
            } else {
                ui.style().visuals.text_color()
            };
            if ui.add(
                Button::new(RichText::new("All").color(all_color).small())
                    .stroke(if all_selected { 
                        Stroke::new(1.0_f32, Color32::from_rgb(42, 222, 192)) 
                    } else { 
                        ui.style().visuals.noninteractive().fg_stroke 
                    })
                    .fill(ui.style().visuals.noninteractive().bg_fill)
            ).clicked() {
                self.selected_category = None;
            }

            // Dynamic category buttons based on what's in notifications
            let categories = self.get_categories();
            for cat in &categories {
                let is_selected = self.selected_category.as_ref() == Some(cat);
                let unread_in_cat = self.unread_count_for_category(cat);
                
                // Color alert category differently
                let cat_color = if cat == "ALERT" {
                    Color32::from_rgb(243, 139, 168) // Red-ish for alerts
                } else if is_selected {
                    Color32::from_rgb(42, 222, 192)
                } else {
                    ui.style().visuals.text_color()
                };
                
                let label = if unread_in_cat > 0 {
                    format!("{} ({})", cat, unread_in_cat)
                } else {
                    cat.clone()
                };
                
                if ui.add(
                    Button::new(RichText::new(label).color(cat_color).small())
                        .stroke(if is_selected { 
                            Stroke::new(1.0_f32, cat_color) 
                        } else { 
                            ui.style().visuals.noninteractive().fg_stroke 
                        })
                        .fill(ui.style().visuals.noninteractive().bg_fill)
                ).clicked() {
                    self.selected_category = Some(cat.clone());
                }
            }
        });
        ui.add_space(5.0);

        // Build filtered index list so mutations apply to the real data
        let query = self.search_query.trim().to_lowercase();
        let selected_cat = self.selected_category.clone();
        let filtered_indices: Vec<usize> = self
            .notifications
            .iter()
            .enumerate()
            .filter_map(|(idx, n)| {
                // Category filter
                if let Some(ref cat) = selected_cat {
                    if &n.notification_type != cat {
                        return None;
                    }
                }
                
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

    /// Get total count of unread notifications
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| n.status == "Unread").count()
    }

    /// Get count of unread notifications for a specific category
    pub fn unread_count_for_category(&self, category: &str) -> usize {
        self.notifications
            .iter()
            .filter(|n| n.status == "Unread" && n.notification_type == category)
            .count()
    }

    /// Get count of unread ALERT notifications
    pub fn alert_count(&self) -> usize {
        self.unread_count_for_category("ALERT")
    }

    /// Get all unique notification categories present in the notifications
    pub fn get_categories(&self) -> Vec<String> {
        let mut categories: Vec<String> = self.notifications
            .iter()
            .map(|n| n.notification_type.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        categories.sort();
        categories
    }

    /// Set category filter (None = show all)
    pub fn set_category_filter(&mut self, category: Option<String>) {
        self.selected_category = category;
    }
}