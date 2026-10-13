use eframe::egui::{Align, Frame, Layout, Margin, NumExt, ScrollArea, Shadow, Ui};
use database::{self, SurrealValue, schema::{LiveTaskPayload, SortDirection, Sortable, Store, TaskNotePayload, User}};
use crate::{PlatformSpawner, Spawner, Displayable, TaskUiActions, tabs::tasks::client_cards::ClientCardData};
use crate::tabs::tasks::ai_task_cards::{AiCardRole, AiTaskCardView};
use crate::tabs::tasks::task_cards::RecommendationSummary;
use crate::ui_tools::{
    controls::{self, Icon, MenuButton},
    icons::svg,
    metrics::Metrics,
    theme,
};
use std::{collections::{BTreeMap, HashMap, HashSet}, f32};
use crossbeam::channel::{Receiver, Sender};
use std::collections::BTreeSet;
use database::schema::{RecordId, RecordIdExt};
use serde::Deserialize;
use serde::Serialize;
use chrono::Utc;
use std::sync::atomic::{AtomicBool, Ordering};

// Global guard to ensure expensive, store-wide notes fetch is only triggered once
static NOTES_FETCH_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
pub struct TaskLayout{
    pub search_inputs: HashMap<String, String>,
    pub task_map: BTreeMap<String, Vec<LiveTaskPayload>>,
    pub assignees: Vec<User>,
    pub open_menu: bool,
    pub action: TaskUiActions,
    pub task: Option<String>,
    pub sort_by: HashMap<String, SortOptions>,
    pub last_sort_field: Option<SortField>,
    pub loading: bool,
    new_status: String,
    user: User,
    search_results: Option<Vec<LiveTaskPayload>>, // Add search results
    pub column_order: Vec<String>,
    pub page: String,
    // True once this page has rendered at least once; used to gate expensive refreshes
    #[serde(skip)]
    pub has_run: bool,
    #[serde(skip)]
    notes_tx: Sender<Vec<TaskNotePayload>>,
    #[serde(skip)]
    notes_rx: Receiver<Vec<TaskNotePayload>>,
    notes: Vec<TaskNotePayload>,
    /// Tracks when notes were last read per task (propagated from SharedContext)
    pub last_read_notes: HashMap<RecordId, chrono::DateTime<chrono::Utc>>,
    /// AI checklist activity per task, for the card's recommendation badge.
    #[serde(skip)]
    pub recommendations: HashMap<RecordId, RecommendationSummary>,
    /// Last description edit per task (edited_at, editor), for the card header.
    #[serde(skip)]
    pub description_changes: HashMap<RecordId, (chrono::DateTime<chrono::Utc>, String)>,
    /// Connected-client cards rendered in the special
    /// `CONNECTED_CLIENTS_KEY` column. Refreshed each frame from
    /// `SharedContext` so newly connected clients appear immediately.
    #[serde(skip)]
    pub client_cards: Vec<ClientCardData>,
    /// Free-text filter applied to the connected-clients column.
    /// Matches case-insensitively against `connection_string`,
    /// `friendly_name`, and the resolved assigned-user name. Kept on
    /// `TaskLayout` (rather than per-page state in `SharedContext`) so
    /// every page that shows a connected-clients column carries its
    /// own filter independently.
    #[serde(skip)]
    pub client_filter: String,
    /// AI-task cards rendered in the `AI_TASKS_KEY` column. Refreshed
    /// each frame from `SharedContext.ai_tasks` / `ai_task_items`.
    #[serde(skip)]
    pub ai_cards: Vec<AiTaskCardView>,
}

/// Sentinel `column_order` key that means "render the connected-clients
/// column here". Picked to never collide with a real status string or
/// username and to serialize cleanly into the user's saved column order.
pub const CONNECTED_CLIENTS_KEY: &str = "__connected_clients__";

/// Sentinel `column_order` key for the AI hands-on-handoff column. Pinned
/// at render time immediately left of the connected-clients column and
/// never persisted into the user's saved order.
pub const AI_TASKS_KEY: &str = "__ai_tasks__";

pub struct LayoutConfig {
    // Valid keys for task_map (statuses or username)
    pub valid_keys: Vec<String>,
    pub key_provider: Box<dyn Fn(&[User]) -> Vec<String>>, // Generate keys from store_users
    pub filter: Box<dyn Fn(&LiveTaskPayload, &Option<User>, &[User], &Store) -> bool>,
    // Whether to call update_assignees
    pub update_assignees: bool,
}

#[derive(Clone, Default, PartialEq, Serialize, Debug, Deserialize, database::SurrealValue)]
pub struct SortOptions {
    pub field: SortField,
    pub direction: SortDirection,
}

#[derive(Clone, Default, PartialEq, Serialize, Debug, Deserialize, database::SurrealValue)]
pub enum SortField {
    #[default]
    Default,
    Date,
    Name,
    /// When the task was completed; the default on the Completed Tasks board.
    Completed,
}

impl TaskLayout {
    /// Starting sort for a board's columns. Completed Tasks opens on most
    /// recently finished; the active boards stay on priority order.
    pub fn default_sort_for(page: &str) -> SortOptions {
        if page == "Completed Tasks" {
            SortOptions { field: SortField::Completed, direction: SortDirection::Desc }
        } else {
            SortOptions::default()
        }
    }

    const COL_W: f32 = 450.0;
    const HEADER_H: f32 = 48.0;
    const SPACER_W: f32 = 6.0;

    pub fn new(
        task_map: BTreeMap<String, Vec<LiveTaskPayload>>,
        column_order: Vec<String>,
        assignees: Vec<User>,
        search_results: Option<Vec<LiveTaskPayload>>,
        page: String,
        current_user: User,
    ) -> Self {
        // log::debug!("Initializing new task layout");
        let (notes_tx, notes_rx) = crossbeam::channel::unbounded();

        let tx = notes_tx.clone();
        let map = task_map.clone();
        if !map.is_empty() {
            PlatformSpawner::spawn(async move {
                for task in map.iter().flat_map(|t| t.1.iter()) {
                    let notes_res = task.get_associated_notes().await;
                    match notes_res {
                        Ok(notes) => { let _ = tx.try_send(notes); },
                        Err(e) => log::error!("Error getting notes: {e:?}"),
                    }
                }
            });
        }

        Self {
            notes: vec![],
            notes_tx,
            notes_rx,
            task_map,
            column_order,
            page,
            assignees,
            search_inputs: HashMap::new(),
            open_menu: false,
            action: TaskUiActions::None,
            task: None,
            sort_by: HashMap::new(),
            last_sort_field: None,
            loading: false,
            new_status: String::new(),
            user: current_user,
            search_results,
            has_run: false,
            last_read_notes: HashMap::new(),
            recommendations: HashMap::new(),
            description_changes: HashMap::new(),
            client_cards: Vec::new(),
            client_filter: String::new(),
            ai_cards: Vec::new(),
        }
    }

    /// Case-insensitive substring match against the three identifying
    /// fields of a connected-client card. Returns `true` when the
    /// filter is empty so the unfiltered list is the default.
    fn client_card_matches_filter(card: &ClientCardData, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();

        if card.client.connection_string.to_lowercase().contains(&q) {
            return true;
        }
        if let Some(fname) = card.client.friendly_name.as_deref() {
            if fname.to_lowercase().contains(&q) {
                return true;
            }
        }
        // Resolve the assigned tech's display name once per card —
        // matches by username, not the raw RecordId, since that's what
        // the operator sees on screen.
        if let Some(ref user_id) = card.client.assigned_user {
            let users = crate::get_database_users();
            if let Some(name) = users
                .iter()
                .find(|u| u.get_id().key_string() == user_id.key_string())
                .map(|u| u.get_name().to_string())
            {
                if name.to_lowercase().contains(&q) {
                    return true;
                }
            }
        }
        false
    }

    pub fn receive(&mut self) {
        while let Ok(notes) = self.notes_rx.try_recv() {
            // Optional: keep only notes for tasks we currently have
            let valid_task_ids: BTreeSet<_> = self
                .task_map
                .values()
                .flatten()
                .map(|t| t.id.clone())
                .collect();

            let mut existing_ids: HashSet<_> = self.notes.iter().map(|n| n.id.clone()).collect();

            self.notes.extend(
                notes.into_iter().filter(|n| {
                    // keep if belongs to a known task and not already present
                    let valid = n.task_id.as_ref().is_some_and(|id| valid_task_ids.contains(id));
                    valid && existing_ids.insert(n.id.clone())
                })
            );
        }

        // Only refetch/stream notes after this page has rendered at least once
        if !self.has_run {
            self.has_run = true;
            log::info!("TaskLayout first receive call (post-first-render)");
            // Ensure global fetch runs only once across all TaskLayout instances
            if NOTES_FETCH_STARTED
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                let tx = self.notes_tx.clone();
                PlatformSpawner::spawn(async move {
                    match TaskNotePayload::get_all_notes_in_my_store(tx.clone()).await {
                        Ok(_) => log::info!("Got all notes for layout"),
                        Err(e) => log::error!("Error getting notes for layout: {e:?}"),
                    }
                });
            } else {
                log::info!("Skipping global notes fetch; already started once");
            }
        }
    }

    pub fn insert_notes(&mut self, notes: Vec<TaskNotePayload>) {
        for note in notes.iter() {
            if self.notes.iter().find(|n| n.id == note.id).is_none() {
                self.notes.push(note.clone());
            }
        }
    }

    pub fn get_notes(&self, task_id: &RecordId) -> Vec<TaskNotePayload> {
        self.notes.iter().filter(|n| n.task_id == Some(task_id.clone())).cloned().collect()
    }

    pub fn update_col_names(&mut self, column_names: Vec<String>) -> &mut Self {
        // If no current order, accept the provided order as-is (lets saved order win)
        if self.column_order.is_empty() {
            self.column_order = column_names;
        } else {
            // Retain only those still present
            let mut retained: Vec<String> = self
                .column_order
                .iter()
                .filter(|n| column_names.contains(*n))
                .cloned()
                .collect();
            // Append new ones
            for n in column_names.iter() {
                if !retained.contains(n) { retained.push(n.clone()); }
            }
            self.column_order = retained;
        }
        self
    }

    fn persist_column_order(&mut self) {
        let mut user = self.user.clone();
        let page = self.page.clone();
        // Render-time-pinned sentinel never enters the saved order.
        let order: Vec<String> = self
            .column_order
            .iter()
            .filter(|k| *k != AI_TASKS_KEY)
            .cloned()
            .collect();
        PlatformSpawner::spawn(async move {
            if let Err(e) = user.save_page_task_columns(&page, order).await {
                log::error!("Failed to save task column layout for {page}: {e:?}");
            }
        });
    }

    fn move_column_left(&mut self, key: &str) {
        if let Some(idx) = self.column_order.iter().position(|k| k == key) {
            if idx > 0 {
                self.column_order.swap(idx, idx - 1);
                self.persist_column_order();
            }
        }
    }

    fn move_column_right(&mut self, key: &str) {
        if let Some(idx) = self.column_order.iter().position(|k| k == key) {
            if idx + 1 < self.column_order.len() {
                self.column_order.swap(idx, idx + 1);
                self.persist_column_order();
            }
        }
    }

    pub fn update_assignees(&mut self, assignees: Vec<User>) -> &mut Self {
        self.assignees = assignees;
        self
    }

    pub fn layout_cols(&mut self, ui: &mut Ui, ui_actions_tx: Sender<TaskUiActions>) {
        let style = ui.style().clone();
        let m = Metrics::get(ui.ctx());

        // Idempotent per-frame pin: AI Tasks sits immediately left of the
        // connected-clients column regardless of any saved/stale order.
        self.column_order.retain(|k| k != AI_TASKS_KEY);
        if self.page == "My Tasks" && !self.ai_cards.is_empty() {
            let pos = self
                .column_order
                .iter()
                .position(|k| k == CONNECTED_CLIENTS_KEY)
                .unwrap_or(0);
            self.column_order.insert(pos, AI_TASKS_KEY.to_string());
        }
        let mut inputs = BTreeSet::new();
        // Defer any column move to after UI borrows are released
        let mut requested_move: Option<(String, i8)> = None; // (-1 left, +1 right)

        //-----------------------------------------------------------------
        // **Grab the viewport height *before* we start nesting UIs**. A child
        // `Ui` created later with `ui.vertical(|col_ui| ...)` does *not* yet
        // know how tall it will be, so `available_height()` there collapses to
        // a tiny value. Capturing it up‑front guarantees we have the full
        // window height for every column.
        //-----------------------------------------------------------------

        let viewport_h = ui.available_height();

        let column_frame = Frame::default()
            .fill(style.visuals.window_fill)
            .inner_margin(Margin::same(6))
            .corner_radius(style.visuals.menu_corner_radius)
            .shadow(Shadow::NONE)
            .stroke(style.visuals.window_stroke);

        let header_frame = Frame::default()
            .fill(style.visuals.window_fill)
            .inner_margin(Margin::same(2))
            .outer_margin(Margin::symmetric(8, 4))
            .corner_radius(style.visuals.window_corner_radius)
            .stroke(style.visuals.window_stroke);

        ScrollArea::horizontal()
            .max_height(viewport_h)
            .max_width(f32::INFINITY)
            .auto_shrink(false)
            .scroll_bar_visibility(eframe::egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in self.column_order.clone().iter().enumerate() {
                    // AI Tasks column: tech section (assigned) + operator
                    // review section, sharing one pinned column.
                    if name == AI_TASKS_KEY {
                        if self.page != "My Tasks" || self.ai_cards.is_empty() {
                            continue;
                        }
                        let mine: Vec<AiTaskCardView> = self
                            .ai_cards
                            .iter()
                            .filter(|c| c.role == AiCardRole::AssignedTech)
                            .cloned()
                            .collect();
                        let review: Vec<AiTaskCardView> = self
                            .ai_cards
                            .iter()
                            .filter(|c| c.role == AiCardRole::Operator)
                            .cloned()
                            .collect();

                        ui.vertical(|col_ui| {
                            let content_w = Self::COL_W - 4.0;
                            header_frame.show(col_ui, |hui| {
                                hui.set_min_width(content_w);
                                hui.set_max_width(content_w);
                                let label_text = if review.is_empty() {
                                    format!("AI Tasks ({})", mine.len())
                                } else {
                                    format!("AI Tasks ({} · {} review)", mine.len(), review.len())
                                };
                                column_title(hui, svg::ROBOT, &label_text);
                            });
                            column_frame.show(col_ui, |fui| {
                                fui.set_min_width(content_w);
                                fui.set_max_width(content_w);
                                let column_h = (viewport_h - Self::HEADER_H).at_least(0.0);
                                fui.set_min_height(column_h);
                                fui.set_max_height(column_h);
                                ScrollArea::vertical()
                                    .id_salt(("ai_tasks_scroll", i))
                                    .max_height(viewport_h - Self::HEADER_H)
                                    .auto_shrink([false; 2])
                                    .show(fui, |sui| {
                                        for card in &mine {
                                            let notes =
                                                self.get_notes(&card.ai_task.task_ref);
                                            let last_read = self
                                                .last_read_notes
                                                .get(&card.ai_task.task_ref)
                                                .copied();
                                            card.display(
                                                sui,
                                                &self.user,
                                                &self.assignees,
                                                notes,
                                                last_read,
                                                &ui_actions_tx,
                                            );
                                        }
                                        if !review.is_empty() {
                                            if !mine.is_empty() {
                                                sui.add_space(m.gap.y);
                                                controls::hairline(sui);
                                            }
                                            controls::IconLabel::new(svg::EYE, "Awaiting your review")
                                                .tone(theme::weak_text(sui))
                                                .small(true)
                                                .show(sui);
                                            for card in &review {
                                                let notes =
                                                    self.get_notes(&card.ai_task.task_ref);
                                                let last_read = self
                                                    .last_read_notes
                                                    .get(&card.ai_task.task_ref)
                                                    .copied();
                                                card.display(
                                                    sui,
                                                    &self.user,
                                                    &self.assignees,
                                                    notes,
                                                    last_read,
                                                    &ui_actions_tx,
                                                );
                                            }
                                        }
                                    });
                            });
                        });
                        ui.add_space(Self::SPACER_W);
                        continue;
                    }
                    // Connected-clients column: My Tasks only; omitted when no
                    // clients pass recency / live-transport filters.
                    if name == CONNECTED_CLIENTS_KEY {
                        if self.page != "My Tasks" {
                            continue;
                        }
                        // Hide the column entirely when every machine is stale / offline.
                        if self.client_cards.is_empty() {
                            continue;
                        }

                        // Pre-compute the visible-after-filter list so
                        // both the header count and the card iteration
                        // see the same set. Cloning indices (cheap)
                        // avoids re-running the case-insensitive
                        // substring + user-name lookup twice.
                        let filter_query = self.client_filter.clone();
                        let visible_cards: Vec<&ClientCardData> = self
                            .client_cards
                            .iter()
                            .filter(|c| Self::client_card_matches_filter(c, &filter_query))
                            .collect();
                        let total = self.client_cards.len();
                        let shown = visible_cards.len();

                        ui.vertical(|col_ui| {
                            let content_w = Self::COL_W - 4.0;
                            header_frame.show(col_ui, |hui| {
                                hui.set_min_width(content_w);
                                hui.set_max_width(content_w);
                                hui.vertical(|hui| {
                                    let label_text = if filter_query.is_empty() {
                                        format!("Connected Clients ({total})")
                                    } else {
                                        format!("Connected Clients ({shown}/{total})")
                                    };
                                    column_title(hui, svg::MONITOR, &label_text);
                                    // Filter input: matches against
                                    // connection_string / friendly_name
                                    // / assigned tech. Width is locked
                                    // to the header content so it lines
                                    // up with the cards below.
                                    controls::TextField::new(&mut self.client_filter)
                                        .hint("Filter by name, connection, tech…")
                                        .icon(svg::FUNNEL_SIMPLE)
                                        .clear_button(true)
                                        .id_salt("connected_clients_filter")
                                        .width(content_w)
                                        .show(hui);
                                });
                            });
                            column_frame.show(col_ui, |fui| {
                                fui.set_min_width(content_w);
                                fui.set_max_width(content_w);
                                let column_h = (viewport_h - Self::HEADER_H).at_least(0.0);
                                fui.set_min_height(column_h);
                                fui.set_max_height(column_h);
                                ScrollArea::vertical()
                                    .id_salt(("connected_clients_scroll", i))
                                    .max_height(viewport_h - Self::HEADER_H)
                                    .auto_shrink([false; 2])
                                    .show(fui, |sui| {
                                        if visible_cards.is_empty() {
                                            controls::empty_state(
                                                sui,
                                                svg::FUNNEL_SIMPLE,
                                                "No clients match your filter",
                                                "",
                                                &[],
                                            );
                                            return;
                                        }
                                        for card in &visible_cards {
                                            card.display_client_card(sui, &ui_actions_tx);
                                            sui.add_space(m.gap.y);
                                        }
                                    });
                            });
                        });
                        ui.add_space(Self::SPACER_W);
                        continue;
                    }
                    if let Some(tasks) = self.task_map.get_mut(name) {
                        if tasks.is_empty() {
                            continue;
                        }

                        ui.vertical(|col_ui| {
                            let default_sort = Self::default_sort_for(&self.page);
                            let sort_by = self
                                .sort_by
                                .entry(name.clone())
                                .or_insert_with(|| default_sort.clone());
                            let direction = &sort_by.direction;
                            match sort_by.field {
                                SortField::Default => tasks.default_sort(direction.clone()),
                                SortField::Date => tasks.sort_by_date(direction.clone()),
                                SortField::Name => tasks.sort_by_name(direction.clone()),
                                SortField::Completed => tasks.sort_by_completed(direction.clone()),
                            };

                            for task in tasks.iter(){
                                inputs.insert(task.task_name.clone());
                                inputs.insert(format!("{}", task.service_number.clone().unwrap_or_default()));
                            }

                            header_frame.show(col_ui, |ui| {
                                // The header frame’s *content* must be narrower by the
                                // 2‑px inner margin on each side, otherwise the outer
                                // stroke can extend past COL_W.
                                let content_w = Self::COL_W - 2.0 * 2.0; // inner margin*2
                                ui.set_min_width(content_w);
                                ui.set_max_width(content_w);
                                let gap = Metrics::get(ui.ctx()).gap.x;

                                ui.push_id(("task_column_header", name.as_str()), |ui| ui.horizontal(|ui|
                                {
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui|
                                    {
                                        // Disable per-column search if global search is active
                                        let search_input = if self.search_results.is_some() {
                                            &mut String::new()
                                        } else {
                                            &mut self.search_inputs.entry(name.clone()).or_default().clone()
                                        };

                                        controls::TextField::new(search_input)
                                            .hint("Search")
                                            .icon(svg::MAGNIFYING_GLASS)
                                            .width(120.0)
                                            .show(ui);

                                        // Update search_inputs only if global search is inactive
                                        if self.search_results.is_none() {
                                            self.search_inputs.insert(name.clone(), search_input.to_string());
                                        }

                                        ui.add_space(gap);

                                        let mut count = 0;
                                        let current_date = Utc::now().date_naive();
                                        let ids = tasks.iter().map(|t| t.id.clone()).collect::<Vec<RecordId>>();

                                        for task in &mut *tasks {
                                            if task.due_date.date_naive() < current_date && !task.completed { count += 1; }
                                        }

                                        if count > 0 {
                                            controls::badge(ui, &count.to_string(), theme::error(ui))
                                                .on_hover_text("Overdue");
                                        }

                                        ui.add_space(gap);
                                        let (_, res) = MenuButton::new(name)
                                            .icon(svg::KANBAN)
                                            .tint(style.visuals.warn_fg_color)
                                            .tip("Column actions")
                                            .keep_open(true)
                                            .show(ui, |ui| {
                                                let mut action = TaskActions::None;
                                                if controls::menu_item(ui, svg::CHECK_SQUARE, "Mark all Complete").clicked() {
                                                    action = TaskActions::MarkComplete;
                                                }
                                                if controls::menu_item(ui, svg::SQUARE, "Mark all Incomplete").clicked() {
                                                    action = TaskActions::MarkIncomplete;
                                                }
                                                if controls::menu_item(ui, svg::CALENDAR_CHECK, "Mark all Due Today").clicked() {
                                                    action = TaskActions::MarkDueToday;
                                                }
                                                controls::hairline(ui);
                                                if controls::menu_item(ui, svg::ARROW_LEFT, "Move column left").clicked() {
                                                    action = TaskActions::MoveLeft;
                                                }
                                                if controls::menu_item(ui, svg::ARROW_RIGHT, "Move column right").clicked() {
                                                    action = TaskActions::MoveRight;
                                                }
                                                action
                                            });

                                        if let Some(response) = res {
                                            match response {
                                                TaskActions::MoveLeft => {
                                                    requested_move = Some((name.clone(), -1));
                                                }
                                                TaskActions::MoveRight => {
                                                    requested_move = Some((name.clone(), 1));
                                                }
                                                other => {
                                                    PlatformSpawner::spawn(async move {
                                                        let result = match other {
                                                            TaskActions::MarkComplete => LiveTaskPayload::update_completed_many(ids, true).await,
                                                            TaskActions::MarkIncomplete => LiveTaskPayload::update_completed_many(ids, false).await,
                                                            TaskActions::MarkDueToday => LiveTaskPayload::update_due_now_many(ids).await,
                                                            _ => Ok(()),
                                                        };
                                                        if let Err(e) = result {
                                                            log::error!("Column action failed: {e:?}");
                                                        }
                                                    });
                                                }
                                            }
                                        }
                                    });

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui|
                                    {
                                        let selected = self
                                            .sort_by
                                            .entry(name.clone())
                                            .or_insert_with(|| Self::default_sort_for(&self.page));
                                        let (icon, tint) = match selected.direction {
                                            SortDirection::Asc => (svg::SORT_ASCENDING, ui.style().visuals.warn_fg_color),
                                            SortDirection::Desc => (svg::SORT_DESCENDING, ui.style().visuals.error_fg_color),
                                        };
                                        let fields = [
                                            (SortField::Default, "Priority"),
                                            (SortField::Name, "Name"),
                                            (SortField::Date, "Date"),
                                            (SortField::Completed, "Completed"),
                                        ];
                                        let label = fields
                                            .iter()
                                            .find(|(field, _)| *field == selected.field)
                                            .map_or("Priority", |(_, label)| *label);

                                        let _ = MenuButton::new(label)
                                            .icon(icon)
                                            .tint(tint)
                                            .tip("Sort this column")
                                            .keep_open(true)
                                            .show(ui, |ui| {
                                                for (field, label) in fields {
                                                    let on = selected.field == field;
                                                    if controls::menu_check_item(ui, None, label, on).clicked() {
                                                        selected.field = field.clone();
                                                        // Toggle the direction if the same field is clicked again
                                                        if self.last_sort_field.as_ref() == Some(&field) {
                                                            selected.direction = match selected.direction {
                                                                SortDirection::Asc => SortDirection::Desc,
                                                                SortDirection::Desc => SortDirection::Asc,
                                                            };
                                                        }
                                                        self.last_sort_field = Some(field);
                                                    }
                                                }
                                            });
                                    });
                                }));
                            });


                            column_frame.show(col_ui, |ui| {
                                // Compensate for 6‑px *inner* margin so the turquoise
                                // stroke remains inside COL_W.
                                let content_w = Self::COL_W - 2.0 * 6.0; // inner margin*2
                                ui.set_min_width(content_w);
                                ui.set_max_width(content_w);

                                let column_h = (viewport_h - Self::HEADER_H).at_least(0.0);
                                ui.set_min_height(column_h);
                                ui.set_max_height(column_h);

                                let row_height = 110.;
                                let total_rows = tasks.len();
                                ui.ctx().options_mut(|o| o.input_options.line_scroll_speed = 80.0);

                                ScrollArea::vertical()
                                    .max_width(Self::COL_W - 20.0)
                                    // .max_height(viewport_h)
                                    .auto_shrink(false)
                                    .id_salt(format!("{}-{i}-scroll", name))
                                    .show_rows(ui, row_height, total_rows, |ui, row_range|
                                {
                                    // ui.scroll_with_delta(Vec2::new(0.0, 300.));
                                    // Retrieve search input for the current context, or default to an empty string.
                                    let search_input = self.search_inputs.get(name).cloned().unwrap_or_default();

                                    // Filter tasks based on search input.
                                    // Assuming tasks is a Vec<TaskPayload> or similar persistent collection
                                    let filtered_indices: Vec<usize> = if !search_input.is_empty() {
                                        tasks
                                            .iter()
                                            .enumerate()
                                            .filter(|(_, task)| {
                                                // Assuming filter_by_task_name returns a Vec<TaskPayload>,
                                                // adapt this to match your filtering logic
                                                task.task_name
                                                    .to_lowercase()
                                                    .contains(&search_input.to_lowercase())
                                            })
                                            .map(|(i, _)| i)
                                            .collect()
                                    } else {
                                        (0..tasks.len()).collect() // All indices when no filter
                                    };

                                    // Iterate only over the rows in the current viewport range
                                    for row in row_range {
                                        if !search_input.is_empty() {
                                            ui.scroll_to_cursor(Some(Align::BOTTOM));
                                        }
                                        if let Some(&task_index) = filtered_indices.get(row) {
                                            if let Some(task) = tasks.get_mut(task_index) {
                                                let notes = self.notes
                                                    .iter()
                                                    .filter(|n|
                                                        if let Some(id) = &n.task_id {
                                                           *id == task.id
                                                        } else {
                                                            false
                                                        }
                                                    )
                                                    .cloned()
                                                    .collect::<Vec<TaskNotePayload>>();

                                                let last_read =
                                                    self.last_read_notes.get(&task.id).copied();
                                                let recommendations = self
                                                    .recommendations
                                                    .get(&task.id)
                                                    .copied()
                                                    .unwrap_or_default();
                                                let description_edit = self
                                                    .description_changes
                                                    .get(&task.id)
                                                    .cloned();

                                                task.display_cards(
                                                    ui,
                                                    &self.user,
                                                    &self.assignees,
                                                    notes,
                                                    ui_actions_tx.clone(),
                                                    last_read,
                                                    recommendations,
                                                    description_edit,
                                                );
                                            }
                                        }
                                    }
                                    if self.loading {
                                        controls::loading_state(ui, "Loading…");
                                    }
                                });
                            });
                        });
                    }
                    if i + 1 < self.column_order.len() {
                        ui.add_space(Self::SPACER_W);
                    }
                }
            });
        });

        // Apply any requested column move now that no UI borrows are active
        if let Some((key, dir)) = requested_move.take() {
            if dir < 0 { self.move_column_left(&key); } else { self.move_column_right(&key); }
        }

        // Mark this page as having rendered at least once so subsequent actions can run
        if !self.has_run {
            self.has_run = true;
        }
    }
}

/// A column title: icon and label in the warn colour.
fn column_title(ui: &mut Ui, icon: Icon, text: &str) {
    controls::IconLabel::new(icon, text)
        .tone(theme::warn(ui))
        .show(ui);
}

#[derive(Clone)]
pub enum TaskActions{
    MarkComplete,
    MarkIncomplete,
    MarkDueToday,
    MoveLeft,
    MoveRight,
    None
}
