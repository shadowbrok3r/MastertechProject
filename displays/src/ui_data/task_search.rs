//! Global task search, shared by the desktop and web menu bars.
//!
//! Local matching answers instantly from `task_index`; a debounced DB query
//! then covers what the client never loads — customer names, phone numbers,
//! and the ~4.6k completed tasks that are only fetched when the Completed tab
//! is opened.
//!
//! Drawn through [`TaskSearchCtx`] rather than `&mut SharedContext`: the menu
//! bars hold a mutable borrow of `current_user` across the whole bar, so the
//! search box has to borrow its fields individually.

use crate::{PlatformSpawner, Spawner, TaskUiActions};
use crossbeam::channel::{Receiver, Sender};
use database::schema::{
    FilterLiveTasks, LiveTaskPayload, QueryMode, RecordId, RecordIdExt, Store, TaskQuery, User,
};
use std::collections::HashMap;
use web_time::{Duration, Instant};

/// Idle time after the last keystroke before the DB is queried.
const DEBOUNCE: Duration = Duration::from_millis(250);
/// Ceiling on server-side results, so a one-letter query can't pull the table.
const RESULT_LIMIT: usize = 200;
const HINT: &str = " Search tasks, customers, phone";

const HELP: &str = "Matches task name, description, service number, customer name and phone.\n\
     Every word has to appear somewhere. Tick Semantic to write it as a question.";

const SEMANTIC_HELP: &str = "Read the query as a question instead of keywords:\n\
     \"tasks for Jane Doe\" — that customer\n\
     \"assigned to josh\" / \"josh's tasks\" — that tech\n\
     \"completed smith\" / \"open smith\" — narrow by state\n\
     Filler words are ignored. Off, every word is matched literally.";

/// How many matches fall on each board, for the count hint by the search box.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct SearchCounts {
    pub open: usize,
    pub completed: usize,
}

impl SearchCounts {
    /// Renders as "3 open · 20 completed", omitting empty halves.
    pub fn label(&self) -> String {
        match (self.open, self.completed) {
            (0, 0) => "no matches".to_string(),
            (o, 0) => format!("{o} open"),
            (0, c) => format!("{c} completed"),
            (o, c) => format!("{o} open · {c} completed"),
        }
    }
}

/// Search state owned by `SharedContext`.
pub struct TaskSearch {
    /// Raw query the current results answer; a reply tagged otherwise is stale.
    pub raw: String,
    /// Raw query the in-flight DB request was issued for.
    pub requested: String,
    pub typed_at: Option<Instant>,
    /// Server-side hits, unioned with local matches each frame.
    pub db_hits: Vec<LiveTaskPayload>,
    pub counts: SearchCounts,
    /// Operator opt-in to intent parsing. Off means every word is matched
    /// literally, which is what most searches want.
    pub semantic: bool,
    pub tx: Sender<(String, Vec<LiveTaskPayload>)>,
    pub rx: Receiver<(String, Vec<LiveTaskPayload>)>,
}

impl Default for TaskSearch {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            raw: String::new(),
            requested: String::new(),
            typed_at: None,
            db_hits: Vec::new(),
            counts: SearchCounts::default(),
            semantic: false,
            tx,
            rx,
        }
    }
}

impl TaskSearch {
    /// Forgets the current query and its results. The mode is a preference,
    /// so it survives.
    pub fn reset(&mut self) {
        self.raw.clear();
        self.requested.clear();
        self.typed_at = None;
        self.db_hits.clear();
        self.counts = SearchCounts::default();
    }

    pub fn mode(&self) -> QueryMode {
        if self.semantic {
            QueryMode::Semantic
        } else {
            QueryMode::Literal
        }
    }
}

/// Everything the search box touches, borrowed field by field.
pub struct TaskSearchCtx<'a> {
    pub state: &'a mut TaskSearch,
    pub input: &'a mut String,
    pub results: &'a mut Option<Vec<LiveTaskPayload>>,
    pub index: &'a mut HashMap<String, LiveTaskPayload>,
    pub tasks: &'a mut Vec<LiveTaskPayload>,
    pub store_users: &'a [User],
    pub store_selection: u64,
    pub actions: &'a Sender<TaskUiActions>,
}

/// The menu-bar search field, its Clear button, and the open/completed count
/// hint. Shared so desktop and web behave identically.
pub fn search_bar(ui: &mut eframe::egui::Ui, mut ctx: TaskSearchCtx<'_>) {
    use eframe::egui::{Key, RichText, TextEdit, Widget};

    let field = TextEdit::singleline(ctx.input)
        .desired_width(200.0)
        .hint_text(HINT)
        .ui(ui)
        .on_hover_text(HELP);

    ui.add_space(5.);
    let cleared = ui.button("Clear").clicked();

    ui.add_space(5.);
    // Switching mode re-parses the same text, so the pending request is stale.
    if ui
        .checkbox(&mut ctx.state.semantic, "Semantic")
        .on_hover_text(SEMANTIC_HELP)
        .changed()
    {
        ctx.state.raw.clear();
        ctx.state.requested.clear();
        ctx.state.db_hits.clear();
    }

    // Enter opens the single best hit rather than leaving the board filtered.
    let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));

    if cleared {
        ctx.input.clear();
    }

    update(&mut ctx);

    if !ctx.input.trim().is_empty() {
        let counts = ctx.state.counts;
        let color = if counts.open + counts.completed == 0 {
            ui.style().visuals.error_fg_color
        } else {
            ui.style().visuals.warn_fg_color
        };
        ui.add_space(6.0);
        ui.label(RichText::new(counts.label()).small().color(color))
            .on_hover_text(
                "Totals across this store. While searching, each board shows both open \
                 and completed matches — completed ones in a Complete column. My Tasks \
                 still only counts work assigned to you; Store Tasks shows everyone's.",
            );
    }

    if submitted {
        if let Some(task) = ctx.results.as_ref().and_then(|r| r.first()).cloned() {
            let _ = ctx.actions.try_send(TaskUiActions::OpenTaskModal(task));
        }
    }
}

/// Recomputes `results` from `input`. Safe to call every frame; the DB query is
/// debounced and de-duplicated.
pub fn update(ctx: &mut TaskSearchCtx<'_>) {
    receive(ctx);

    let raw = ctx.input.trim().to_string();
    if raw.is_empty() {
        *ctx.results = None;
        ctx.state.reset();
        return;
    }

    let query = TaskQuery::parse_with(&raw, ctx.state.mode());
    if query.is_empty() {
        *ctx.results = Some(Vec::new());
        ctx.state.counts = SearchCounts::default();
        return;
    }

    // A changed query restarts the debounce and re-opens the DB request.
    if ctx.state.raw != raw {
        ctx.state.raw = raw.clone();
        ctx.state.typed_at = Some(Instant::now());
        ctx.state.requested.clear();
        ctx.state.db_hits.clear();
    }

    let users: Vec<User> = ctx.store_users.to_vec();
    let resolve = move |id: &RecordId| -> Option<String> {
        users
            .iter()
            .find(|u| u.get_id().key_string() == id.key_string())
            .map(|u| u.get_name().to_string())
    };

    // Local hits render immediately; DB hits are unioned in on arrival.
    let mut merged: Vec<LiveTaskPayload> = ctx
        .index
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .filter_by_query(&query, &resolve);

    for task in ctx.state.db_hits.iter() {
        if !merged
            .iter()
            .any(|t| t.id.key_string() == task.id.key_string())
        {
            merged.push(task.clone());
        }
    }

    ctx.state.counts = SearchCounts {
        open: merged.iter().filter(|t| !t.completed).count(),
        completed: merged.iter().filter(|t| t.completed).count(),
    };
    *ctx.results = Some(merged);

    let debounced = ctx
        .state
        .typed_at
        .is_some_and(|t| t.elapsed() >= DEBOUNCE);
    if debounced && ctx.state.requested != raw {
        ctx.state.requested = raw.clone();
        let store = Store::from_presta_store_id(&ctx.store_selection.to_string())
            .as_str()
            .to_string();
        let tx = ctx.state.tx.clone();
        PlatformSpawner::spawn(async move {
            match query.search(&store, RESULT_LIMIT).await {
                Ok(tasks) => {
                    let _ = tx.try_send((raw, tasks));
                }
                Err(e) => log::error!("task search failed: {e:?}"),
            }
        });
    }
}

/// Folds arrived DB results in, discarding replies to superseded queries.
/// Results also land in the task index so cards, notes, and staged edits
/// resolve for completed tasks the client had never loaded.
fn receive(ctx: &mut TaskSearchCtx<'_>) {
    while let Ok((raw, tasks)) = ctx.state.rx.try_recv() {
        if raw != ctx.state.raw {
            continue;
        }
        for task in &tasks {
            let key = task.id.key_string();
            if !ctx.index.contains_key(&key) {
                ctx.tasks.push(task.clone());
            }
            ctx.index.insert(key, task.clone());
        }
        ctx.state.db_hits = tasks;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_label_covers_each_combination() {
        assert_eq!(SearchCounts { open: 0, completed: 0 }.label(), "no matches");
        assert_eq!(SearchCounts { open: 3, completed: 0 }.label(), "3 open");
        assert_eq!(
            SearchCounts { open: 0, completed: 20 }.label(),
            "20 completed"
        );
        assert_eq!(
            SearchCounts { open: 3, completed: 20 }.label(),
            "3 open · 20 completed"
        );
    }

    fn ctx<'a>(
        state: &'a mut TaskSearch,
        input: &'a mut String,
        results: &'a mut Option<Vec<LiveTaskPayload>>,
        index: &'a mut HashMap<String, LiveTaskPayload>,
        tasks: &'a mut Vec<LiveTaskPayload>,
        actions: &'a Sender<TaskUiActions>,
    ) -> TaskSearchCtx<'a> {
        TaskSearchCtx {
            state,
            input,
            results,
            index,
            tasks,
            store_users: &[],
            store_selection: 1,
            actions,
        }
    }

    fn indexed(names: &[&str], completed: &[bool]) -> HashMap<String, LiveTaskPayload> {
        names
            .iter()
            .zip(completed)
            .map(|(name, done)| {
                let mut t = LiveTaskPayload::default();
                t.task_name = name.to_string();
                t.completed = *done;
                (t.id.key_string(), t)
            })
            .collect()
    }

    #[test]
    fn completed_matches_are_not_hidden_by_an_open_one() {
        let (actions, _rx) = crossbeam::channel::unbounded();
        let mut state = TaskSearch::default();
        let mut input = "smith".to_string();
        let mut results = None;
        let mut index = indexed(
            &["Jane Smith - 1", "Jane Smith - 2", "Jane Smith - 3"],
            &[false, true, true],
        );
        let mut tasks = Vec::new();

        update(&mut ctx(
            &mut state,
            &mut input,
            &mut results,
            &mut index,
            &mut tasks,
            &actions,
        ));

        assert_eq!(state.counts.open, 1);
        assert_eq!(state.counts.completed, 2);
        assert_eq!(results.as_ref().map(Vec::len), Some(3));
    }

    #[test]
    fn empty_input_clears_results() {
        let (actions, _rx) = crossbeam::channel::unbounded();
        let mut state = TaskSearch::default();
        let mut input = String::new();
        let mut results = Some(vec![LiveTaskPayload::default()]);
        let mut index = HashMap::new();
        let mut tasks = Vec::new();

        update(&mut ctx(
            &mut state,
            &mut input,
            &mut results,
            &mut index,
            &mut tasks,
            &actions,
        ));

        assert!(results.is_none());
        assert!(state.raw.is_empty());
    }

    #[test]
    fn scope_keyword_restricts_results_only_in_semantic_mode() {
        let (actions, _rx) = crossbeam::channel::unbounded();
        let mut results = None;
        let mut tasks = Vec::new();

        // Semantic: "completed" is a scope filter, so only the finished one.
        let mut state = TaskSearch::default();
        state.semantic = true;
        let mut input = "completed smith".to_string();
        let mut index = indexed(&["Jane Smith - 1", "Jane Smith - 2"], &[false, true]);
        update(&mut ctx(
            &mut state,
            &mut input,
            &mut results,
            &mut index,
            &mut tasks,
            &actions,
        ));
        assert_eq!(state.counts.open, 0);
        assert_eq!(state.counts.completed, 1);

        // Literal (the default): "completed" is a word neither name contains.
        let mut state = TaskSearch::default();
        assert!(!state.semantic, "literal is the default");
        let mut input = "completed smith".to_string();
        let mut index = indexed(&["Jane Smith - 1", "Jane Smith - 2"], &[false, true]);
        update(&mut ctx(
            &mut state,
            &mut input,
            &mut results,
            &mut index,
            &mut tasks,
            &actions,
        ));
        assert_eq!(state.counts.open, 0);
        assert_eq!(state.counts.completed, 0);
    }

    #[test]
    fn literal_mode_still_finds_both_states_for_a_customer() {
        // The reported bug: one open task must not hide the finished ones.
        let (actions, _rx) = crossbeam::channel::unbounded();
        let mut state = TaskSearch::default();
        let mut input = "smith".to_string();
        let mut results = None;
        let mut index = indexed(
            &["Jane Smith - 1", "Jane Smith - 2", "Jane Smith - 3"],
            &[false, true, true],
        );
        let mut tasks = Vec::new();

        update(&mut ctx(
            &mut state,
            &mut input,
            &mut results,
            &mut index,
            &mut tasks,
            &actions,
        ));

        assert_eq!(state.counts.open, 1);
        assert_eq!(state.counts.completed, 2);
    }
}
