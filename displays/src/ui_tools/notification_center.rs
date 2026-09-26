use database::schema::assistant::{
    complete_assistant_task, snooze_notification, TYPE_MORNING_BRIEF, TYPE_OVERDUE, TYPE_PART_REQUEST, TYPE_REMINDER,
};
use database::schema::{utilities::NotificationMod, Notification, RecordId};
use eframe::egui::*;

use crate::{ui_tools::{icons, theme}, PlatformSpawner, Spawner};

/// Notification types that carry an assistant task a person can finish or snooze.
const ACTIONABLE_TYPES: [&str; 3] = [TYPE_REMINDER, TYPE_OVERDUE, TYPE_PART_REQUEST];

/// A click on a notification row, applied after the list is drawn.
enum RowAction {
    Done(usize),
    Snooze(usize, chrono::DateTime<chrono::Utc>),
    OpenBrief(usize),
}

/// Known notification categories
pub const NOTIFICATION_CATEGORIES: &[&str] = &[
    "All",
    "Task Update",
    "Task Created",
    "AI Attention",
    "AI Followup",
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
    /// Morning brief text shown in its own window.
    #[serde(skip)]
    pub brief: Option<String>,
    /// Notification behind the open brief; marked read when the window closes.
    #[serde(skip)]
    pub brief_id: Option<RecordId>,
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
        if self.notifications.is_empty() {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No notifications").heading().weak());
            });
            return;
        }

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
                    theme::error(ui)
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
            let mut actions: Vec<RowAction> = Vec::new();
            scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
                for row in row_range {
                    if let Some(&idx) = indices.get(row) {
                        let notification = &mut self.notifications[idx];
                        let actionable = notification.status == "Unread"
                            && notification.task.is_some()
                            && ACTIONABLE_TYPES.contains(&notification.notification_type.as_str());
                        let is_brief = notification.notification_type == TYPE_MORNING_BRIEF;
                        
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
                                    if actionable {
                                        let now = chrono::Utc::now();
                                        ui.menu_button(icons::menu_item(icons::SNOOZE, "Snooze"), |ui| {
                                            if ui.button("1 hour").clicked() {
                                                actions.push(RowAction::Snooze(idx, now + chrono::Duration::hours(1)));
                                                ui.close();
                                            }
                                            if ui.button("Next open morning").clicked() {
                                                let until = database::schema::task_schedule::next_open_morning(now);
                                                actions.push(RowAction::Snooze(idx, until));
                                                ui.close();
                                            }
                                        });
                                        if ui.small_button(icons::menu_item(icons::CHECK, "Done")).clicked() {
                                            actions.push(RowAction::Done(idx));
                                        }
                                    }
                                });
                            });

                            ui.separator();
                            if is_brief {
                                let first = notification.notification_description.lines().next().unwrap_or("");
                                ui.horizontal(|ui| {
                                    ui.label(first);
                                    if ui.small_button("Open brief").clicked() {
                                        actions.push(RowAction::OpenBrief(idx));
                                    }
                                });
                            } else {
                                crate::ui_tools::show_notification(
                                    ui,
                                    &notification.notification_description,
                                    &task_names,
                                    ui_actions_tx.clone(),
                                    &tasks,
                                );
                            }
                        })
                        .inner;
                    }
                }
            });
            for action in actions {
                self.apply_row_action(action);
            }
        });
    }

    fn apply_row_action(&mut self, action: RowAction) {
        match action {
            RowAction::OpenBrief(idx) => {
                if let Some(n) = self.notifications.get(idx) {
                    self.show_brief(n.id.clone(), n.notification_description.clone());
                }
            }
            RowAction::Done(idx) => {
                let Some(n) = self.notifications.get_mut(idx) else { return };
                n.status = "Read".to_string();
                let mut notif = n.clone();
                let task = n.task.clone();
                PlatformSpawner::spawn(async move {
                    if let Some(task) = task
                        && let Err(e) = complete_assistant_task(&task).await
                    {
                        log::warn!("notification Done: completing the task failed: {e}");
                    }
                    let _ = notif.mark_notification(true).await;
                });
            }
            RowAction::Snooze(idx, until) => {
                let Some(n) = self.notifications.get_mut(idx) else { return };
                n.status = "Snoozed".to_string();
                let id: RecordId = n.id.clone();
                PlatformSpawner::spawn(async move {
                    if let Err(e) = snooze_notification(&id, until).await {
                        log::warn!("notification snooze failed: {e}");
                    }
                });
            }
        }
    }

    /// Opens the brief window on `text`.
    pub fn show_brief(&mut self, id: RecordId, text: String) {
        self.brief = Some(text);
        self.brief_id = Some(id);
    }

    /// Marks the brief notification read once its window closes.
    fn close_brief(&mut self) {
        self.brief = None;
        let Some(id) = self.brief_id.take() else { return };
        let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) else { return };
        if n.status != "Read" {
            n.status = "Read".to_string();
            let mut notif = n.clone();
            PlatformSpawner::spawn(async move {
                let _ = notif.mark_notification(true).await;
            });
        }
    }

    /// The morning brief window, while one is open.
    pub fn brief_window(&mut self, ctx: &Context) {
        let Some(text) = self.brief.clone() else { return };
        let mut open = true;
        Window::new(format!("{} Morning brief", icons::MORNING_BRIEF))
            .id(Id::new("morning_brief_window"))
            .open(&mut open)
            .default_width(460.0)
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for line in text.lines() {
                        ui.label(line);
                    }
                });
            });
        if !open {
            self.close_brief();
        }
    }

    /// Opens the notification window on the unread list, optionally filtered
    /// to one category.
    pub fn open_unread(&mut self, category: Option<String>) {
        self.show_notifications = true;
        self.read_notifications = false;
        self.selected_category = category;
    }

    pub fn set_notifications(&mut self, notifications: Vec<Notification>) {
        self.notifications = notifications;
    }

    /// Replaces the row with the same id, or adds it first.
    pub fn apply_update(&mut self, notification: Notification) {
        match self.notifications.iter_mut().find(|n| n.id == notification.id) {
            Some(existing) => *existing = notification,
            None => self.notifications.insert(0, notification),
        }
    }

    pub fn apply_delete(&mut self, notification: Notification) {
        self.notifications.retain(|n| n.id != notification.id);
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