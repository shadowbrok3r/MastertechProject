use chrono::{DateTime, Utc};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::schema::{
    LiveTaskPayload, Priority, RecordId, RecordIdExt, Status, Store, TaskPayload, TaskQuery, User,
};

/// Keeps the tasks `matches` accepts, ordered by fuzzy score of `name` against
/// the raw query so the closest name lands first. Ties fall back to input
/// order, which is already date-sorted upstream.
fn rank<T: Clone>(
    tasks: &[T],
    query: &TaskQuery,
    name: impl Fn(&T) -> &String,
    assignee: impl Fn(&T) -> &RecordId,
    matches: impl Fn(&T, Option<&str>) -> bool,
    assignee_name: &dyn Fn(&RecordId) -> Option<String>,
) -> Vec<T> {
    if query.is_empty() {
        return Vec::new();
    }
    let matcher = SkimMatcherV2::default().ignore_case();
    let mut scored: Vec<(i64, usize, T)> = tasks
        .iter()
        .enumerate()
        .filter_map(|(i, task)| {
            let resolved = assignee_name(assignee(task));
            if !matches(task, resolved.as_deref()) {
                return None;
            }
            let score = matcher.fuzzy_match(name(task), &query.raw).unwrap_or(0);
            Some((score, i, task.clone()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, task)| task).collect()
}

pub trait FilterTasks {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<TaskPayload>;
    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<TaskPayload>;
    /// Tasks matching `query`, best fuzzy match first. Completion state is
    /// decided by `query.scope` alone — an open match never hides a
    /// completed one.
    ///
    /// `assignee_name` resolves a task's assignee to a display name so
    /// "assigned to <name>" can be answered locally; return `None` when
    /// unknown.
    fn filter_by_query(
        &self,
        query: &TaskQuery,
        assignee_name: &dyn Fn(&RecordId) -> Option<String>,
    ) -> Vec<TaskPayload>;
}

pub trait FilterLiveTasks {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<LiveTaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<LiveTaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<LiveTaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<LiveTaskPayload>;
    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<LiveTaskPayload>;
    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<LiveTaskPayload>;
    /// Tasks matching `query`, best fuzzy match first. Completion state is
    /// decided by `query.scope` alone — an open match never hides a
    /// completed one.
    ///
    /// `assignee_name` resolves a task's assignee to a display name so
    /// "assigned to <name>" can be answered locally; return `None` when
    /// unknown.
    fn filter_by_query(
        &self,
        query: &TaskQuery,
        assignee_name: &dyn Fn(&RecordId) -> Option<String>,
    ) -> Vec<LiveTaskPayload>;
}

impl FilterLiveTasks for Vec<LiveTaskPayload> {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.assignee == assignee.get_id())
            .cloned()
            .collect()
    }

    fn filter_by_completion(&self, completed: bool) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.completed == completed)
            .cloned()
            .collect()
    }

    fn filter_by_status(&self, status: &Status) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.status == *status)
            .cloned()
            .collect()
    }

    fn filter_by_priority(&self, priority: &Priority) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.priority == *priority)
            .cloned()
            .collect()
    }

    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| task.due_date >= date.into())
            .cloned()
            .collect()
    }

    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<LiveTaskPayload> {
        self.into_iter()
            .filter(|task| {
                assignee.get_store() == *store && task.assignee.key_string() == assignee.get_id().key_string()
            })
            .cloned()
            .collect()
    }

    fn filter_by_query(
        &self,
        query: &TaskQuery,
        assignee_name: &dyn Fn(&RecordId) -> Option<String>,
    ) -> Vec<LiveTaskPayload> {
        rank(
            self,
            query,
            |t| &t.task_name,
            |t| &t.assignee,
            |t, name| query.matches_local(t, name),
            assignee_name,
        )
    }
}

impl FilterTasks for Vec<TaskPayload> {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.assignee == assignee.get_id())
            .cloned()
            .collect()
    }

    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.completed == completed)
            .cloned()
            .collect()
    }

    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.status == *status)
            .cloned()
            .collect()
    }

    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.priority == *priority)
            .cloned()
            .collect()
    }

    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.due_date >= date.into())
            .cloned()
            .collect()
    }

    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| {
                assignee.get_store() == *store && task.assignee.key_string() == assignee.get_id().key_string()
            })
            .cloned()
            .collect()
    }

    fn filter_by_query(
        &self,
        query: &TaskQuery,
        assignee_name: &dyn Fn(&RecordId) -> Option<String>,
    ) -> Vec<TaskPayload> {
        rank(
            self,
            query,
            |t| &t.task_name,
            |t| &t.assignee,
            |t, name| query.matches_local(&t.clone().into(), name),
            assignee_name,
        )
    }
}
