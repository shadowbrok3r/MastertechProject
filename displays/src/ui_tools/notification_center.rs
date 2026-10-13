use database::schema::assistant::{
    TYPE_MORNING_BRIEF, TYPE_OVERDUE, TYPE_PART_REQUEST, TYPE_REMINDER, complete_assistant_task,
    snooze_notification,
};
use database::schema::{Notification, RecordId, utilities::NotificationMod};
use eframe::egui::*;

use crate::{
    PlatformSpawner, Spawner,
    ui_tools::{
        controls::{self, IconBox, IconButton, IconLabel, MenuButton, Metrics, Tokens},
        icons::svg::{self, Icon},
    },
};

/// Notification types that carry an assistant task a person can finish or snooze.
const ACTIONABLE_TYPES: [&str; 3] = [TYPE_REMINDER, TYPE_OVERDUE, TYPE_PART_REQUEST];

/// Width of the search field.
const SEARCH_W: f32 = 240.0;
/// Height `show_rows` assumes for a notification row.
const ROW_H: f32 = 100.0;

/// The icon a notification category shows.
fn category_icon(category: &str) -> Icon {
    match category {
        "ALERT" => svg::WARNING,
        "Task Update" => svg::ARROWS_CLOCKWISE,
        "Task Created" => svg::PLUS_CIRCLE,
        "AI Attention" => svg::ROBOT,
        "AI Followup" => svg::SPARKLE,
        "Admin" => svg::MEGAPHONE,
        "System" => svg::GEAR,
        TYPE_MORNING_BRIEF => svg::SUN_HORIZON,
        TYPE_REMINDER => svg::ALARM,
        TYPE_OVERDUE => svg::CLOCK_COUNTDOWN,
        TYPE_PART_REQUEST => svg::PACKAGE,
        _ => svg::BELL,
    }
}

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
        tasks: &Vec<database::schema::LiveTaskPayload>,
    ) {
        let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
        ui.add_space(m.gap.y);
        if self.notifications.is_empty() {
            controls::empty_state(
                ui,
                svg::BELL_SLASH,
                "No notifications",
                "You're all caught up.",
                &[],
            );
            return;
        }

        ui.vertical_centered(|ui| {
            let width = SEARCH_W.min(ui.available_width());
            controls::search_field(ui, &mut self.search_query, "Search notifications…", width);
        });

        ui.add_space(m.gap.y);
        ui.horizontal_wrapped(|ui| {
            if controls::pill(
                ui,
                svg::SQUARES_FOUR,
                "All",
                self.selected_category.is_none(),
            )
            .clicked()
            {
                self.selected_category = None;
            }
            for cat in &self.get_categories() {
                let is_selected = self.selected_category.as_ref() == Some(cat);
                let unread_in_cat = self.unread_count_for_category(cat);
                let label = if unread_in_cat > 0 {
                    format!("{} ({})", cat, unread_in_cat)
                } else {
                    cat.clone()
                };
                if controls::pill(ui, category_icon(cat), &label, is_selected).clicked() {
                    self.selected_category = Some(cat.clone());
                }
            }
        });
        ui.add_space(m.gap.y);

        let query = self.search_query.trim().to_lowercase();
        let selected_cat = self.selected_category.clone();
        let filtered_indices: Vec<usize> = self
            .notifications
            .iter()
            .enumerate()
            .filter_map(|(idx, n)| {
                if let Some(ref cat) = selected_cat
                    && &n.notification_type != cat
                {
                    return None;
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
            if controls::pill(ui, svg::ENVELOPE_SIMPLE, "Unread", !self.read_notifications)
                .clicked()
            {
                self.read_notifications = false;
            }
            if controls::pill(ui, svg::CHECKS, "Read", self.read_notifications).clicked() {
                self.read_notifications = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (icon, all_label) = if self.read_notifications {
                    (svg::ENVELOPE_SIMPLE, "Mark all unread")
                } else {
                    (svg::CHECKS, "Mark all read")
                };
                if controls::TextButton::new(all_label)
                    .icon(icon)
                    .kind(controls::ButtonKind::Ghost)
                    .show(ui)
                    .on_hover_text("Apply to all currently filtered notifications")
                    .clicked()
                {
                    let make_read = !self.read_notifications;
                    self.mark_all_by_indices(&filtered_indices, make_read);
                }
            });
        });
        ui.add_space(m.gap.y / 2.0);

        let total_rows = filtered_indices.len();
        let scroll_area = ScrollArea::vertical().auto_shrink(false);
        ui.ctx()
            .options_mut(|o| o.input_options.line_scroll_speed = 15.0);

        if total_rows == 0 {
            let title = if self.read_notifications {
                "Nothing read yet"
            } else {
                "No unread notifications"
            };
            controls::empty_state(ui, svg::TRAY, title, "Try another category or search.", &[]);
            return;
        }

        ui.scope(|ui| {
            let indices = filtered_indices.clone();
            let mut actions: Vec<RowAction> = Vec::new();
            scroll_area.show_rows(ui, ROW_H, total_rows, |ui, row_range| {
                for row in row_range {
                    if let Some(&idx) = indices.get(row) {
                        let notification = &mut self.notifications[idx];
                        let actionable = notification.status == "Unread"
                            && notification.task.is_some()
                            && ACTIONABLE_TYPES.contains(&notification.notification_type.as_str());
                        let is_brief = notification.notification_type == TYPE_MORNING_BRIEF;

                        Frame::new()
                        .corner_radius(CornerRadius::same(t.radius_md))
                        .fill(t.raised)
                        .stroke(Stroke::new(1.0, t.card_border))
                        .inner_margin(Margin::same(m.gap.x as i8))
                        .outer_margin(Margin::symmetric(0, (m.gap.y / 2.0) as i8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let kind = notification.notification_type.as_str();
                                let tone = if kind == "ALERT" { t.error } else { t.tertiary };
                                IconLabel::new(category_icon(kind), kind).tone(tone).show(ui);

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let read = notification.status == "Read";
                                    let (icon, tip) = if read {
                                        (svg::ENVELOPE_SIMPLE, "Mark unread")
                                    } else {
                                        (svg::CHECKS, "Mark read")
                                    };
                                    let toggle = IconButton::new(icon, IconBox::Sm).tip(tip).show(ui);

                                    if toggle.clicked() {
                                        let mut notif = notification.clone();
                                        if read {
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
                                        let _ = MenuButton::new("").icon(svg::ALARM).tip("Snooze").show(ui, |ui| {
                                            if controls::menu_item(ui, svg::CLOCK, "1 hour").clicked() {
                                                actions.push(RowAction::Snooze(idx, now + chrono::Duration::hours(1)));
                                                ui.close();
                                            }
                                            if controls::menu_item(ui, svg::SUN_HORIZON, "Next open morning").clicked() {
                                                let until = database::schema::task_schedule::next_open_morning(now);
                                                actions.push(RowAction::Snooze(idx, until));
                                                ui.close();
                                            }
                                        });
                                        if controls::icon_button(ui, svg::CHECK, IconBox::Sm, false, "Done").clicked() {
                                            actions.push(RowAction::Done(idx));
                                        }
                                    }
                                });
                            });

                            controls::hairline(ui);
                            if is_brief {
                                let first = notification.notification_description.lines().next().unwrap_or("");
                                ui.horizontal(|ui| {
                                    ui.label(first);
                                    if controls::icon_text_button(ui, svg::SUN_HORIZON, "Open brief").clicked() {
                                        actions.push(RowAction::OpenBrief(idx));
                                    }
                                });
                            } else {
                                crate::ui_tools::show_notification(
                                    ui,
                                    &notification.notification_description,
                                    task_names,
                                    ui_actions_tx.clone(),
                                    tasks,
                                );
                            }
                        });
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
                let Some(n) = self.notifications.get_mut(idx) else {
                    return;
                };
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
                let Some(n) = self.notifications.get_mut(idx) else {
                    return;
                };
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
        let Some(id) = self.brief_id.take() else {
            return;
        };
        let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) else {
            return;
        };
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
        let Some(text) = self.brief.clone() else {
            return;
        };
        let mut open = true;
        controls::icon_window(ctx, svg::SUN_HORIZON, "Morning brief")
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
        match self
            .notifications
            .iter_mut()
            .find(|n| n.id == notification.id)
        {
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
                let should_change =
                    (make_read && n.status != "Read") || (!make_read && n.status == "Read");
                if should_change {
                    let mut clone = n.clone();
                    n.status = if make_read {
                        "Read".to_string()
                    } else {
                        "Unread".to_string()
                    };
                    PlatformSpawner::spawn(async move {
                        let _ = clone.mark_notification(make_read).await;
                    });
                }
            }
        }
    }

    /// Get total count of unread notifications
    pub fn unread_count(&self) -> usize {
        self.notifications
            .iter()
            .filter(|n| n.status == "Unread")
            .count()
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
        let mut categories: Vec<String> = self
            .notifications
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
