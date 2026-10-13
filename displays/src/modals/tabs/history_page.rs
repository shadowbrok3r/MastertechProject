//! Task History Page - displays the history of changes made to a task

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

use chrono::{DateTime, Duration, Utc};
use database::schema::{RecordIdExt, SYSTEM_AUTHOR, TaskHistory, TaskHistoryLoad, User};
use eframe::egui::{Grid, Ui};

use super::page_scroll;
use crate::ui_tools::{
    controls::{self, IconLabel, Metrics, Section, Tokens},
    icons::svg,
    theme,
};

/// Identical diffs within this span of a cluster's first row are copies of one change.
const DUPLICATE_WINDOW: Duration = Duration::minutes(15);

/// One field's old and new value, ready to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldChange {
    pub field: String,
    pub old: String,
    pub new: String,
}

/// A history row's diff, ready to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffView {
    Changes(Vec<FieldChange>),
    Empty,
    Missing,
    Unexpected(String),
}

/// One change as displayed: a history row with its duplicate copies folded in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryEntry {
    pub username: String,
    pub created_at: DateTime<Utc>,
    pub diff: DiffView,
    /// Other usernames that recorded a copy, set only when the author is uncertain.
    pub uncertain_with: Vec<String>,
}

/// Rows recording one change; at most one of them is a writer row.
struct Cluster<'a> {
    rows: Vec<&'a TaskHistory>,
    first_at: DateTime<Utc>,
    open: bool,
}

impl Cluster<'_> {
    fn has_writer(&self) -> bool {
        self.rows.iter().any(|row| row.from_writer)
    }
}

/// Cached entries for one history list.
#[derive(Clone)]
struct Prepared {
    fingerprint: u64,
    entries: Arc<Vec<HistoryEntry>>,
}

/// Displays the task history page with collapsible entries showing diffs
pub fn display_history_page(ui: &mut Ui, history: &TaskHistoryLoad) {
    ui.vertical(|ui| {
        if history.rows.is_empty() {
            controls::empty_state(
                ui,
                svg::CLOCK_COUNTER_CLOCKWISE,
                "No history yet",
                "No history records found for this task.",
                &[],
            );
            return;
        }

        let entries = prepared_entries(ui, history);
        let m = Metrics::get(ui.ctx());
        page_scroll(ui, "task_history_page", |ui| {
            for (idx, entry) in entries.iter().enumerate() {
                render_history_entry(ui, entry, idx);
                ui.add_space(m.gap.y);
            }
        });
    });
}

/// Entries for `history`, rebuilt only when its rows, its names or the loaded user count change.
fn prepared_entries(ui: &Ui, history: &TaskHistoryLoad) -> Arc<Vec<HistoryEntry>> {
    let id = ui.id().with("task_history_entries");
    let cached = ui.data(|d| d.get_temp::<Prepared>(id));
    // A busy user list reuses the cache, or builds without it and caches nothing.
    let Ok(users) = database::STORE_USERS.try_lock() else {
        return cached
            .map(|cached| cached.entries)
            .unwrap_or_else(|| Arc::new(build_entries(history, &[])));
    };
    let fingerprint = fingerprint(history, users.len());
    if let Some(cached) = cached
        && cached.fingerprint == fingerprint
    {
        return cached.entries;
    }
    let entries = Arc::new(build_entries(history, &users));
    drop(users);
    ui.data_mut(|d| {
        d.insert_temp(
            id,
            Prepared {
                fingerprint,
                entries: entries.clone(),
            },
        )
    });
    entries
}

/// Hash of the rows' ids, the resolved names and `user_count`.
fn fingerprint(history: &TaskHistoryLoad, user_count: usize) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    user_count.hash(&mut hasher);
    history.usernames.len().hash(&mut hasher);
    for row in &history.rows {
        row.id.key_string().hash(&mut hasher);
    }
    hasher.finish()
}

fn build_entries(history: &TaskHistoryLoad, users: &[User]) -> Vec<HistoryEntry> {
    let names = username_lookup(users, &history.rows, &history.usernames);
    history_entries(&history.rows, &names)
}

/// Maps `user` keys to usernames from the authors of `history`, then `users`, then `resolved`.
pub(crate) fn username_lookup(
    users: &[User],
    history: &[TaskHistory],
    resolved: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut names: HashMap<String, String> = history
        .iter()
        .filter(|row| !row.username.is_empty() && row.username != SYSTEM_AUTHOR)
        .map(|row| (row.user.key_string(), row.username.clone()))
        .collect();
    for user in users {
        names.insert(user.get_id().key_string(), user.get_username().to_string());
    }
    names.extend(
        resolved
            .iter()
            .map(|(key, name)| (key.clone(), name.clone())),
    );
    names
}

/// The display text for `raw`, resolving user keys in [`TaskHistory::USER_FIELDS`] to usernames.
pub(crate) fn display_value(field: &str, raw: &str, names: &HashMap<String, String>) -> String {
    if TaskHistory::USER_FIELDS.contains(&field) {
        let key = raw.strip_prefix("user:").unwrap_or(raw);
        if let Some(name) = names.get(key) {
            return name.clone();
        }
    }
    raw.to_string()
}

/// Newest-first entries with duplicate copies collapsed and user keys resolved.
pub(crate) fn history_entries(
    history: &[TaskHistory],
    names: &HashMap<String, String>,
) -> Vec<HistoryEntry> {
    let mut clusters = collapse_duplicates(history);
    clusters.sort_by_key(|cluster| std::cmp::Reverse(cluster.first_at));
    clusters
        .iter()
        .map(|cluster| {
            let author = cluster
                .rows
                .iter()
                .find(|row| row.from_writer)
                .copied()
                .unwrap_or(cluster.rows[0]);
            let mut others: Vec<String> = Vec::new();
            for row in &cluster.rows {
                if row.username != author.username && !others.contains(&row.username) {
                    others.push(row.username.clone());
                }
            }
            let uncertain = !author.from_writer && !others.is_empty();
            HistoryEntry {
                username: author.username.clone(),
                created_at: cluster.first_at,
                diff: diff_view(&author.diff, names),
                uncertain_with: if uncertain { others } else { Vec::new() },
            }
        })
        .collect()
}

/// Clusters identical diffs within [`DUPLICATE_WINDOW`] until a row reverts one of their fields.
fn collapse_duplicates(history: &[TaskHistory]) -> Vec<Cluster<'_>> {
    let mut rows: Vec<(&TaskHistory, DateTime<Utc>)> = history
        .iter()
        .map(|row| (row, row.created_at.into()))
        .collect();
    rows.sort_by_key(|(_, at)| *at);

    let mut clusters: Vec<Cluster> = Vec::new();
    for (row, at) in rows {
        let joined = clusters.iter().rposition(|cluster| {
            cluster.open
                && cluster.rows[0].diff == row.diff
                && at - cluster.first_at <= DUPLICATE_WINDOW
                && !(row.from_writer && cluster.has_writer())
        });
        for (idx, cluster) in clusters.iter_mut().enumerate() {
            if Some(idx) != joined && cluster.open && reverts(&row.diff, &cluster.rows[0].diff) {
                cluster.open = false;
            }
        }
        match joined {
            Some(idx) => clusters[idx].rows.push(row),
            None => clusters.push(Cluster {
                rows: vec![row],
                first_at: at,
                open: true,
            }),
        }
    }
    clusters
}

/// True when `later` sets any field of `earlier` back to its old value.
fn reverts(later: &serde_json::Value, earlier: &serde_json::Value) -> bool {
    let (Some(later), Some(earlier)) = (later.as_object(), earlier.as_object()) else {
        return false;
    };
    earlier.iter().any(|(field, change)| {
        later
            .get(field)
            .is_some_and(|other| other != change && other.get("new") == change.get("old"))
    })
}

/// Converts a stored diff into display rows.
fn diff_view(diff: &serde_json::Value, names: &HashMap<String, String>) -> DiffView {
    match diff {
        serde_json::Value::Object(map) if map.is_empty() => DiffView::Empty,
        serde_json::Value::Object(map) => DiffView::Changes(
            map.iter()
                .map(|(field, change)| FieldChange {
                    field: format_field_name(field),
                    old: display_value(field, &side_text(change.get("old")), names),
                    new: display_value(field, &side_text(change.get("new")), names),
                })
                .collect(),
        ),
        serde_json::Value::Null => DiffView::Missing,
        other => DiffView::Unexpected(format!("{other:?}")),
    }
}

/// One side of a field change as text; `-` when absent.
fn side_text(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s.clone(),
        None | Some(serde_json::Value::Null) => "-".to_string(),
        Some(other) => other.to_string(),
    }
}

/// Renders a single history entry as a collapsing header
fn render_history_entry(ui: &mut Ui, entry: &HistoryEntry, idx: usize) {
    let header_text = format!("{} - {}", entry.username, entry.created_at.date_naive());

    Section::new(format!("history_{}", idx), &header_text)
        .icon(svg::USER_CIRCLE)
        .default_open(idx == 0)
        .show(ui, |ui| {
            render_diff_content(ui, &entry.diff);
            if !entry.uncertain_with.is_empty() {
                let t = Tokens::get(ui.ctx());
                let text = format!(
                    "Author uncertain; also recorded by {}",
                    entry.uncertain_with.join(", ")
                );
                IconLabel::new(svg::USERS, &text)
                    .small(true)
                    .tone(t.text_dim)
                    .wrap(true)
                    .show(ui);
            }
        });
}

/// Renders the diff content inside a history entry
fn render_diff_content(ui: &mut Ui, diff: &DiffView) {
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    match diff {
        DiffView::Changes(changes) => {
            Grid::new(ui.next_auto_id())
                .num_columns(3)
                .spacing([2.0 * m.gap.x, m.gap.y])
                .striped(true)
                .min_col_width(150.)
                .max_col_width(200.)
                .show(ui, |ui| {
                    controls::icon_label(ui, svg::TAG, "Field");
                    controls::icon_label(ui, svg::MINUS_CIRCLE, "Old Value");
                    controls::icon_label(ui, svg::PLUS_CIRCLE, "New Value");
                    ui.end_row();

                    for change in changes {
                        ui.colored_label(t.text_dim, &change.field);
                        ui.colored_label(theme::error(ui), or_empty(&change.old));
                        ui.colored_label(theme::success(ui), or_empty(&change.new));
                        ui.end_row();
                    }
                });
        }
        DiffView::Empty => {
            controls::icon_label(ui, svg::INFO, "No changes recorded.");
        }
        DiffView::Missing => {
            controls::icon_label(ui, svg::INFO, "No diff data available.");
        }
        DiffView::Unexpected(raw) => {
            let text = format!("Unexpected diff format: {raw}");
            controls::status_label(ui, svg::WARNING, &text, theme::warn(ui));
        }
    }
}

fn or_empty(value: &str) -> &str {
    if value.is_empty() { "(empty)" } else { value }
}

/// Converts snake_case field names to Title Case for display
fn format_field_name(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use database::schema::RecordId;
    use serde_json::json;

    const TASK: &str = "d78ef283";

    fn at(h: u32, m: u32, s: u32, ms: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 8, h, m, s).unwrap() + Duration::milliseconds(ms as i64)
    }

    fn row(
        username: &str,
        user_key: &str,
        when: DateTime<Utc>,
        diff: serde_json::Value,
    ) -> TaskHistory {
        TaskHistory {
            id: RecordId::new(
                "task_history",
                format!("{username}-{}", when.timestamp_millis()),
            ),
            task_id: RecordId::new("task", TASK),
            user: RecordId::new("user", user_key),
            username: username.to_string(),
            diff,
            created_at: when.into(),
            from_writer: false,
        }
    }

    fn writer(
        username: &str,
        user_key: &str,
        when: DateTime<Utc>,
        diff: serde_json::Value,
    ) -> TaskHistory {
        TaskHistory {
            from_writer: true,
            ..row(username, user_key, when, diff)
        }
    }

    fn status(old: &str, new: &str) -> serde_json::Value {
        json!({ "status": { "old": old, "new": new } })
    }

    fn assignee(old: &str, new: &str) -> serde_json::Value {
        json!({ "assignee": { "old": old, "new": new } })
    }

    fn entries(history: &[TaskHistory]) -> Vec<HistoryEntry> {
        history_entries(history, &HashMap::new())
    }

    fn user(key: &str, email: &str) -> User {
        let mut user = User::default();
        user.id = RecordId::new("user", key);
        user.set_email(email);
        user
    }

    #[test]
    fn copies_from_every_client_collapse_to_the_earliest_writer() {
        // Five copies of one status change within 1.3 s.
        let history = vec![
            row(
                "joshua.adams",
                "j",
                at(17, 27, 31, 335),
                status("follow up", "REPO"),
            ),
            row(
                "jacob.hardy",
                "h",
                at(17, 27, 31, 206),
                status("follow up", "REPO"),
            ),
            row(
                "joshua.adams",
                "j",
                at(17, 27, 30, 643),
                status("follow up", "REPO"),
            ),
            row(
                "joshua.adams",
                "j",
                at(17, 27, 30, 535),
                status("follow up", "REPO"),
            ),
            row(
                "logan.lees",
                "l",
                at(17, 27, 30, 83),
                status("follow up", "REPO"),
            ),
        ];
        let out = entries(&history);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].username, "logan.lees");
        assert_eq!(out[0].created_at, at(17, 27, 30, 83));
        assert_eq!(
            out[0].uncertain_with,
            vec!["joshua.adams".to_string(), "jacob.hardy".to_string()]
        );
    }

    #[test]
    fn a_writer_row_wins_attribution_and_clears_the_uncertainty() {
        let history = vec![
            row("joshua.adams", "j", at(9, 0, 0, 500), status("Todo", "QC")),
            writer("tyler.naylor", "t", at(9, 0, 1, 0), status("Todo", "QC")),
        ];
        let out = entries(&history);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].username, "tyler.naylor");
        assert!(out[0].uncertain_with.is_empty());
    }

    #[test]
    fn two_writer_rows_are_two_edits_even_inside_the_window() {
        // The change back between them went unrecorded.
        let history = vec![
            writer("tyler.naylor", "t", at(9, 0, 0, 0), status("Todo", "QC")),
            row("joshua.adams", "j", at(9, 0, 0, 400), status("Todo", "QC")),
            writer("jacob.hardy", "h", at(9, 4, 0, 0), status("Todo", "QC")),
            row("logan.lees", "l", at(9, 4, 0, 300), status("Todo", "QC")),
        ];
        let out = entries(&history);
        assert_eq!(out.len(), 2);
        assert_eq!(
            (out[0].username.as_str(), out[0].created_at),
            ("jacob.hardy", at(9, 4, 0, 0))
        );
        assert_eq!(
            (out[1].username.as_str(), out[1].created_at),
            ("tyler.naylor", at(9, 0, 0, 0))
        );
        assert!(out.iter().all(|entry| entry.uncertain_with.is_empty()));
    }

    #[test]
    fn copies_from_a_lagging_client_inside_the_window_still_collapse() {
        // Second copy 5m32s after the first.
        let history = vec![
            row("jacob.hardy", "h", at(15, 58, 7, 728), assignee("a", "b")),
            row("tyler.naylor", "t", at(16, 3, 40, 453), assignee("a", "b")),
        ];
        assert_eq!(entries(&history).len(), 1);
    }

    #[test]
    fn a_change_made_again_after_a_revert_stays_separate() {
        // X to Y, back to X, then X to Y again.
        let history = vec![
            row("joshua.adams", "j", at(16, 41, 29, 248), assignee("x", "y")),
            row("logan.lees", "l", at(16, 41, 29, 331), assignee("x", "y")),
            row("jacob.hardy", "h", at(16, 45, 0, 0), assignee("y", "x")),
            row("tyler.naylor", "t", at(16, 45, 1, 0), assignee("y", "x")),
            row("joshua.adams", "j", at(16, 50, 0, 0), assignee("x", "y")),
            row("logan.lees", "l", at(16, 50, 0, 400), assignee("x", "y")),
        ];
        let out = entries(&history);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].created_at, at(16, 50, 0, 0));
        assert_eq!(out[1].created_at, at(16, 45, 0, 0));
        assert_eq!(out[2].created_at, at(16, 41, 29, 248));
    }

    #[test]
    fn the_same_change_outside_the_window_stays_separate() {
        let history = vec![
            row("logan.lees", "l", at(9, 0, 0, 0), status("Todo", "QC")),
            row("logan.lees", "l", at(9, 30, 0, 0), status("Todo", "QC")),
        ];
        assert_eq!(entries(&history).len(), 2);
    }

    #[test]
    fn different_changes_in_the_same_second_stay_separate() {
        let history = vec![
            row("logan.lees", "l", at(9, 0, 0, 0), status("Todo", "QC")),
            row("logan.lees", "l", at(9, 0, 0, 10), assignee("x", "y")),
        ];
        assert_eq!(entries(&history).len(), 2);
    }

    #[test]
    fn one_author_recording_twice_is_not_uncertain() {
        let history = vec![
            row("joshua.adams", "j", at(9, 0, 0, 0), status("Todo", "QC")),
            row("joshua.adams", "j", at(9, 0, 0, 200), status("Todo", "QC")),
        ];
        let out = entries(&history);
        assert_eq!(out.len(), 1);
        assert!(out[0].uncertain_with.is_empty());
    }

    #[test]
    fn assignee_keys_resolve_to_usernames() {
        let users = vec![
            user("aam998y60shwah3gr9hv", "jacob.hardy@pclaptops.com"),
            user("jm9a7l3v32gsiccr7pgw", "logan.lees@pclaptops.com"),
        ];
        let names = username_lookup(&users, &[], &HashMap::new());
        assert_eq!(
            display_value("assignee", "aam998y60shwah3gr9hv", &names),
            "jacob.hardy"
        );
        assert_eq!(
            display_value("assignee", "user:jm9a7l3v32gsiccr7pgw", &names),
            "logan.lees"
        );
        assert_eq!(
            display_value("assignee", "unknownkey", &names),
            "unknownkey"
        );
        assert_eq!(
            display_value("status", "aam998y60shwah3gr9hv", &names),
            "aam998y60shwah3gr9hv"
        );
    }

    #[test]
    fn names_read_from_the_user_table_cover_inactive_users_and_win() {
        let users = vec![user("k1", "stale.name@pclaptops.com")];
        let resolved = HashMap::from([
            ("k1".to_string(), "renamed".to_string()),
            ("gone".to_string(), "former.tech".to_string()),
        ]);
        let names = username_lookup(&users, &[], &resolved);
        assert_eq!(display_value("assignee", "k1", &names), "renamed");
        assert_eq!(display_value("assigned_by", "gone", &names), "former.tech");
    }

    #[test]
    fn history_authors_name_users_missing_from_the_user_list() {
        let history = vec![
            row(
                "tyler.naylor",
                "rkq1542nn2vhbaqqlled",
                at(9, 0, 0, 0),
                assignee("rkq1542nn2vhbaqqlled", "other"),
            ),
            row(SYSTEM_AUTHOR, "other", at(9, 5, 0, 0), status("Todo", "QC")),
        ];
        let names = username_lookup(&[], &history, &HashMap::new());
        let out = history_entries(&history, &names);
        let DiffView::Changes(changes) = &out[1].diff else {
            panic!("expected changes");
        };
        assert_eq!(changes[0].field, "Assignee");
        assert_eq!(changes[0].old, "tyler.naylor");
        assert_eq!(changes[0].new, "other");
    }
}
