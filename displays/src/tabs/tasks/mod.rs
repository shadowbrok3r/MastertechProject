use database::schema::{
    ConnectedClient, FilterLiveTasks, LiveTaskPayload, RecordIdExt, Status, Store,
};
use eframe::egui::{Color32, Spinner, Ui, Widget};
use crate::app_state::SharedContext;
use crate::tabs::tasks::client_cards::{should_show_connected_client_in_summaries, ClientCardData};
use crate::tabs::tasks::task_layout::CONNECTED_CLIENTS_KEY;
use std::collections::BTreeMap;
use task_layout::TaskLayout;

pub mod task_cards;
pub mod task_layout;
pub mod interactable;
pub mod client_cards;
pub mod ai_task_cards;
pub mod pending;
pub mod complete_button;

/// Column key for completed tasks surfaced by a search on a board that
/// otherwise only shows open work. Matches `Status::Complete.as_str()`.
pub const COMPLETE_KEY: &str = "Complete";

/// Completed tasks assigned to `user`, for the search-only Complete column.
fn completed_for_assignee(
    tasks: &[LiveTaskPayload],
    user: &database::schema::User,
) -> Vec<LiveTaskPayload> {
    tasks
        .to_vec()
        .filter_by_assignee(user)
        .into_iter()
        .filter(|task| task.completed)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::User;

    fn assigned(user: &User, completed: bool, name: &str) -> LiveTaskPayload {
        let mut t = LiveTaskPayload::default();
        t.task_name = name.to_string();
        t.assignee = user.get_id();
        t.completed = completed;
        if completed {
            t.status = Status::Complete;
        }
        t
    }

    /// The bug this column exists for: a customer with one open task and
    /// several finished ones must not collapse to just the open one.
    #[test]
    fn completed_column_keeps_every_finished_task() {
        let me = User::default();
        let tasks = vec![
            assigned(&me, false, "Jane Smith - 1"),
            assigned(&me, true, "Jane Smith - 2"),
            assigned(&me, true, "Jane Smith - 3"),
        ];

        let done = completed_for_assignee(&tasks, &me);
        assert_eq!(done.len(), 2, "both finished tasks belong in the column");
        assert!(done.iter().all(|t| t.completed));
    }

    #[test]
    fn completed_column_excludes_other_peoples_work() {
        let me = User::default();
        let mut someone_else = User::default();
        someone_else.id = database::schema::random_record_id("user");

        let tasks = vec![
            assigned(&me, true, "mine"),
            assigned(&someone_else, true, "theirs"),
        ];

        let done = completed_for_assignee(&tasks, &me);
        assert_eq!(done.len(), 1, "My Tasks stays scoped to the signed-in user");
        assert_eq!(done[0].task_name, "mine");
    }

    #[test]
    fn completed_column_is_empty_when_nothing_is_finished() {
        let me = User::default();
        let tasks = vec![assigned(&me, false, "open")];
        // An empty column is skipped at render time, so no Complete column shows.
        assert!(completed_for_assignee(&tasks, &me).is_empty());
    }
}

impl SharedContext {
    pub fn render_layout(&mut self, ui: &mut Ui, page: &str) {
        ui.ctx().request_repaint();
        if self.store_users.is_empty() {
            ui.vertical_centered(|ui| {
                ui.label("Loading...");
                Spinner::new()
                    .size(50.0)
                    .color(Color32::from_rgb(150, 10, 150))
                    .ui(ui);
            });
            return;
        }
        // Initialize layout_configs
        self.init_layout_configs();

        // Ensure layout_configs exists
        let layout_configs = match &self.layout_configs {
            Some(configs) => configs,
            None => {
                log::warn!("Layout configs not initialized for {}", page);
                return;
            }
        };

        // Get or update the layout
        let Some(config) = layout_configs.get(page) else { return; };

        let store_selection = Store::from_presta_store_id(&self.store_selection.to_string());
        let current_user = self.current_user.as_ref().cloned().unwrap_or_default();
        let is_privileged = current_user.is_admin();
        let current_user_id = current_user.get_id();

        let transport_live = |c: &ConnectedClient| -> bool {
            self.web_console_layout
                .ws_clients
                .get(&c.connection_string)
                .map(|w| {
                    use crate::tabs::admin_console::client_interface::TransportKind;
                    // TCP and relay-tunnel sessions prove liveness in-band.
                    if w.transport.kind() != TransportKind::WebSocket {
                        w.is_connected
                    } else {
                        w.is_connected && w.last_pong_time.is_some()
                    }
                })
                .unwrap_or(false)
        };

        let my_tasks_show_clients_column = page == "My Tasks"
            && self.clients.iter().any(|c| {
                let assigned_here = is_privileged
                    || c.assigned_user
                        .as_ref()
                        .is_some_and(|u| *u == current_user_id);
                assigned_here && should_show_connected_client_in_summaries(c, transport_live(c))
            });

        // Always rebuild task_map to reflect current tasks
        let mut map = BTreeMap::new();
        let mut ordered_keys = Vec::new();

        // Use search_results if present, otherwise use all tasks
        let tasks_to_filter = self.search_results.clone().unwrap_or_else(|| {
            self.task_index.values().cloned().collect::<Vec<LiveTaskPayload>>()
        });

        // A search answers across the whole table, so the board's own
        // completion rule is lifted for the duration: a customer with one open
        // and twenty finished tasks has to show all twenty-one, not just the
        // open one. Grouping still follows the board.
        let searching = self.search_results.is_some();

        if page == "My Tasks" {
            if my_tasks_show_clients_column {
                map.insert(CONNECTED_CLIENTS_KEY.to_string(), Vec::new());
                ordered_keys.push(CONNECTED_CLIENTS_KEY.to_string());
            }

            // Collect tasks for each status and include empty columns so saved order persists
            for status_str in &config.valid_keys {
                let status = Status::from_str(status_str);
                let filtered = tasks_to_filter
                    .clone()
                    .filter_by_status(&status)
                    .filter_by_assignee(&current_user)
                    .into_iter()
                    .filter(|task| !task.completed)
                    .collect::<Vec<LiveTaskPayload>>();

                map.insert(status_str.clone(), filtered);
                ordered_keys.push(status_str.clone());
            }

            // valid_keys never contains Complete, so searched-up completed
            // tasks need a column of their own. Empty columns don't render.
            if searching {
                let done = completed_for_assignee(&tasks_to_filter, &current_user);
                map.insert(COMPLETE_KEY.to_string(), done);
                ordered_keys.push(COMPLETE_KEY.to_string());
            }
        } else {
            for user in self.store_users.iter() {
                let by_user = tasks_to_filter
                    .clone()
                    .filter_by_assignee(user);
                let filtered = if searching {
                    by_user.filter_by_store(user, &store_selection)
                } else {
                    by_user
                        .filter_by_completion(page == "Completed Tasks")
                        .filter_by_store(user, &store_selection)
                };

                if !filtered.is_empty() {
                    let username = user.get_username().to_string();
                    map.insert(username.clone(), filtered);
                    ordered_keys.push(username);
                }
            }
        }

        // Apply user's saved column order for this page, if any
        if let Some(user) = self.current_user.as_ref() {
            if let Some(saved) = user.get_page_task_columns(page) {
                let mut applied: Vec<String> = Vec::new();
                for k in saved.iter() {
                    if map.contains_key(k) && !applied.contains(k) {
                        applied.push(k.clone());
                    }
                }
                for k in ordered_keys.iter() {
                    if !applied.contains(k) {
                        applied.push(k.clone());
                    }
                }
                ordered_keys = applied;
            }
        }

        // When searching, render the best-matching page's layout ephemerally in this tab
        if self.search_results.is_some() {
            // Helper to build the map for a target page and return (map, ordered_keys, total_count)
            let build_for_page = |target_page: &str| -> (BTreeMap<String, Vec<LiveTaskPayload>>, Vec<String>, usize) {
                let mut target_map = BTreeMap::new();
                let mut target_ordered_keys = Vec::new();
                if target_page == "My Tasks" {
                    let mut temp_entries: Vec<(String, Vec<LiveTaskPayload>)> = Vec::new();
                    if let Some(cfg) = layout_configs.get(target_page) {
                        for status_str in &cfg.valid_keys {
                            let status = Status::from_str(status_str);
                            let filtered = tasks_to_filter
                                .clone()
                                .filter_by_status(&status)
                                .filter_by_assignee(&current_user)
                                .into_iter()
                                .filter(|task| !task.completed)
                                .collect::<Vec<LiveTaskPayload>>();
                            if !filtered.is_empty() {
                                temp_entries.push((status_str.clone(), filtered));
                            }
                        }
                    }
                    let done = completed_for_assignee(&tasks_to_filter, &current_user);
                    if !done.is_empty() {
                        temp_entries.push((COMPLETE_KEY.to_string(), done));
                    }
                    temp_entries.sort_by(|(a, _), (b, _)| match (a.as_str(), b.as_str()) {
                        ("Todo", _) => std::cmp::Ordering::Less,
                        (_, "Todo") => std::cmp::Ordering::Greater,
                        ("In Repair", _) => std::cmp::Ordering::Less,
                        (_, "In Repair") => std::cmp::Ordering::Greater,
                        (COMPLETE_KEY, _) => std::cmp::Ordering::Greater,
                        (_, COMPLETE_KEY) => std::cmp::Ordering::Less,
                        _ => a.cmp(b),
                    });
                    for (status_str, filtered) in temp_entries {
                        target_ordered_keys.push(status_str.clone());
                        target_map.insert(status_str, filtered);
                    }
                } else {
                    for user in self.store_users.iter() {
                        let filtered = tasks_to_filter
                            .clone()
                            .filter_by_assignee(user)
                            .filter_by_store(user, &store_selection);
                        if !filtered.is_empty() {
                            let username = user.get_username().to_string();
                            target_ordered_keys.push(username.clone());
                            target_map.insert(username, filtered);
                        }
                    }
                }

                let total_count: usize = target_map.values().map(|v| v.len()).sum();
                (target_map, target_ordered_keys, total_count)
            };

            // Stay put whenever this board has hits of its own: hopping to
            // whichever board had the most would hide the other set entirely,
            // and the menu-bar count hint already says where the rest are.
            let (_, _, here) = build_for_page(page);
            let mut best_page: Option<(&str, BTreeMap<String, Vec<LiveTaskPayload>>, Vec<String>)> = None;
            if here == 0 {
                let pages = ["My Tasks", "Store Tasks", "Completed Tasks"];
                let mut best_count = 0usize;
                for p in pages.iter() {
                    let (p_map, p_order, p_count) = build_for_page(p);
                    if p_count > 0 && (best_page.is_none() || p_count > best_count) {
                        best_count = p_count;
                        best_page = Some((p, p_map, p_order));
                    }
                }
            }

            if let Some((best, target_map, mut target_ordered_keys)) = best_page {
                // If the best page is the current page, continue with the normal path below
                if best != page {
                    // Apply user's saved order to target_ordered_keys if any (for the best page)
                    if let Some(user) = self.current_user.as_ref() {
                        if let Some(saved) = user.get_page_task_columns(best) {
                            let mut applied: Vec<String> = Vec::new();
                            for k in saved.iter() {
                                if target_map.contains_key(k) && !applied.contains(k) { applied.push(k.clone()); }
                            }
                            for k in target_ordered_keys.iter() {
                                if !applied.contains(k) { applied.push(k.clone()); }
                            }
                            target_ordered_keys = applied;
                        }
                    }

                    // Ephemeral render of the best-page layout inside the current tab
                    let mut temp_layout = TaskLayout::new(
                        target_map,
                        target_ordered_keys,
                        self.store_users.clone(),
                        self.search_results.clone(),
                        best.to_string(),
                        current_user.clone(),
                    );
                    if let Some(cfg) = layout_configs.get(best) {
                        if cfg.update_assignees {
                            temp_layout.update_assignees(self.store_users.clone());
                        }
                    }
                    temp_layout.layout_cols(ui, self.ui_actions_tx.clone());
                    return;
                }
            }
        }

        // Build My Tasks connected-client cards before borrowing `task_layouts` mutably.
        let mut my_tasks_client_cards: Vec<ClientCardData> = Vec::new();
        if page == "My Tasks" {
            my_tasks_client_cards = self
                .clients
                .iter()
                .filter(|c| {
                    let assigned_here = is_privileged
                        || c.assigned_user
                            .as_ref()
                            .is_some_and(|u| *u == current_user_id);
                    assigned_here
                        && should_show_connected_client_in_summaries(c, transport_live(c))
                })
                .map(|c| {
                    let ws = self
                        .web_console_layout
                        .ws_clients
                        .get(&c.connection_string);
                    let system_info = ws.and_then(|w| w.resource_monitor.latest_sysinfo.clone());
                    let is_ws_connected = ws
                        .map(|w| {
                            use crate::tabs::admin_console::client_interface::TransportKind;
                            // TCP and relay-tunnel sessions prove liveness in-band.
                            if w.transport.kind() != TransportKind::WebSocket {
                                w.is_connected
                            } else {
                                w.is_connected && w.last_pong_time.is_some()
                            }
                        })
                        .unwrap_or(false);
                    let active_session_id = self
                        .web_console_layout
                        .active_diagnostic_sessions
                        .get(&c.connection_string)
                        .cloned();
                    ClientCardData {
                        client: c.clone(),
                        system_info,
                        ai_active: active_session_id.is_some(),
                        active_session_id,
                        computer_data: None,
                        linked_task: None,
                        is_ws_connected,
                    }
                })
                .collect();

            my_tasks_client_cards.sort_by(|a, b| {
                let a_mine = a.client.assigned_user.as_ref()
                    .is_some_and(|u| u.key_string() == current_user_id.key_string());
                let b_mine = b.client.assigned_user.as_ref()
                    .is_some_and(|u| u.key_string() == current_user_id.key_string());
                b_mine.cmp(&a_mine)
                    .then_with(|| b.is_ws_connected.cmp(&a.is_ws_connected))
                    .then_with(|| {
                        let a_name = a.client.friendly_name.as_deref().unwrap_or(&a.client.connection_string);
                        let b_name = b.client.friendly_name.as_deref().unwrap_or(&b.client.connection_string);
                        a_name.cmp(b_name)
                    })
            });
        }

        // Build the AI Tasks column cards: tech section (my handoffs, held
        // until closed) and operator review section (handoffs I requested
        // that await follow-up). The grace window only drives the done banner.
        let mut my_tasks_ai_cards: Vec<crate::tabs::tasks::ai_task_cards::AiTaskCardView> =
            Vec::new();
        if page == "My Tasks" {
            use crate::tabs::tasks::ai_task_cards::{AiCardRole, AiTaskCardView};
            use database::schema::AiTaskStatus;
            const GRACE: web_time::Duration = web_time::Duration::from_secs(3);
            self.ai_task_done_grace.retain(|_, t| t.elapsed() < GRACE);

            for task in self.ai_tasks.values() {
                let key = task.id.key_string();
                let is_tech = task.assignee == current_user_id;
                let is_op = task.requested_by == current_user_id;
                let in_grace = self.ai_task_done_grace.contains_key(&key);

                let role = match task.status {
                    AiTaskStatus::Open if is_tech => AiCardRole::AssignedTech,
                    // Review card wins for an operator who is also the tech.
                    AiTaskStatus::AwaitingFollowup if is_op => AiCardRole::Operator,
                    // Card persists past the grace window so the assignee keeps
                    // a route to the handback until it is closed.
                    AiTaskStatus::AwaitingFollowup if is_tech => AiCardRole::AssignedTech,
                    _ => continue,
                };

                let mut items: Vec<database::schema::AiTaskItem> = self
                    .ai_task_items
                    .values()
                    .filter(|i| i.ai_task_ref.key_string() == key)
                    .cloned()
                    .collect();
                items.sort_by_key(|i| i.position);

                my_tasks_ai_cards.push(AiTaskCardView {
                    linked_task: self.task_index.get(&task.task_ref.key_string()).cloned(),
                    ai_task: task.clone(),
                    items,
                    role,
                    in_grace,
                });
            }
            my_tasks_ai_cards.sort_by(|a, b| {
                b.ai_task.created_at.cmp(&a.ai_task.created_at)
            });
        }

        // Update or create layout
        let layout = self.task_layouts.entry(page.to_string()).or_insert_with(|| {
            let mut layout = TaskLayout::new(
                map.clone(),
                ordered_keys.clone(),
                self.store_users.clone(),
                self.search_results.clone(),
                page.to_string(),
                current_user.clone(),
            );
            // Try to load and apply user's saved order for this page
            if let Some(user) = self.current_user.as_ref() {
                if let Some(saved) = user.get_page_task_columns(page) {
                    layout.update_col_names(saved);
                }
            }
            if config.update_assignees {
                layout.update_assignees(self.store_users.clone());
            }
            layout
        });

        // Update existing layout
        layout.task_map = map;
        // Preserve current layout order; just merge in any new/removed columns
        layout.update_col_names(ordered_keys);
        // Propagate last_read_notes from SharedContext
        layout.last_read_notes = self.last_read_notes.clone();
        layout.client_cards = my_tasks_client_cards;
        layout.ai_cards = my_tasks_ai_cards;

        // Render the layout
        layout.layout_cols(ui, self.ui_actions_tx.clone());
    }
}