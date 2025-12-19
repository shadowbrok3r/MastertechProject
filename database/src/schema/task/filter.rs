
use std::cmp::Reverse;

use chrono::{DateTime, Utc};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::schema::{LiveTaskPayload, Priority, RecordIdExt, Status, Store, TaskPayload, User};

pub trait FilterTasks {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<TaskPayload>;
    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<TaskPayload>;
    /// Filters a list of tasks by their name based on a fuzzy search input.
    /// # Parameters
    /// - `search`: An iterator over items of type `S` where `S` can be referenced as a string slice.
    /// - `search_input`: A string representing the search input to filter tasks by.
    ///
    /// # Returns
    /// A vector of `TaskPayload` containing the filtered tasks.
    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        name: T,
        search_input: String,
    ) -> Vec<TaskPayload>;
}

pub trait FilterLiveTasks {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<LiveTaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<LiveTaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<LiveTaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<LiveTaskPayload>;
    fn filter_by_date(&self, date: DateTime<Utc>) -> Vec<LiveTaskPayload>;
    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<LiveTaskPayload>;
    /// Filters a list of tasks by their name based on a fuzzy search input.
    /// # Parameters
    /// - `search`: An iterator over items of type `S` where `S` can be referenced as a string slice.
    /// - `search_input`: A string representing the search input to filter tasks by.
    ///
    /// # Returns
    /// A vector of `TaskPayload` containing the filtered tasks.
    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug> (
        &self,
        name: T,
        search_input: String,
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

    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug> (
        &self,
        search: T,
        search_input: String,
    ) -> Vec<LiveTaskPayload> {
        // Create a fuzzy matcher with default settings, ignoring case
        let matcher = SkimMatcherV2::default().ignore_case();

        // If search input is empty, return no tasks
        if search_input.trim().is_empty() {
            return vec![];
        }

        // Pre-filter search to reduce fuzzy match calls
        let search_input_lower = search_input.to_lowercase();
        let match_results = search
            .into_iter()
            .filter(|s| s.as_ref().to_lowercase().contains(&search_input_lower))
            .filter_map(|s| {
                let s_str = s.as_ref();
                // Use fuzzy matching, with fallback for single-letter inputs
                matcher.fuzzy_indices(s_str, &search_input).map(|(score, indices)| {
                    let adjusted_score = if search_input.len() == 1 {
                        score.max(1) // Ensure single-letter matches have a positive score
                    } else {
                        score
                    };
                    (s, adjusted_score, indices)
                })
            })
            .collect::<Vec<_>>();

        // Create a map of task IDs to tasks for O(1) lookups
        let task_map: std::collections::HashMap<_, _> = self
            .iter()
            .map(|task| (task.id.key_string(), task))
            .collect();

        // Collect tasks with their best match scores
        let mut task_scores = vec![];
        let mut seen_ids = std::collections::HashSet::with_capacity(self.len());

        for (output, input_score, _) in match_results {
            let output_str = output.as_ref();
            for task in self.iter() {
                let task_id = task.id.key_string().clone();
                if seen_ids.contains(&task_id) {
                    continue;
                }
                let task_name = task.task_name.as_str();
                // Compute fuzzy match score for task_name only
                if let Some((task_score, _)) = matcher.fuzzy_indices(task_name, output_str) {
                    let combined_score = task_score.max(input_score);
                    task_scores.push((task_id.clone(), combined_score));
                    seen_ids.insert(task_id);
                }
            }
        }

        // Sort tasks by score in descending order
        task_scores.sort_by_key(|&(_, score)| Reverse(score));

        // Collect unique tasks, avoiding clones
        task_scores
            .into_iter()
            .filter_map(|(task_id, _)| task_map.get(&task_id).map(|task| *task))
            .cloned()
            .collect::<Vec<LiveTaskPayload>>()
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

    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug> (
        &self,
        search: T,
        search_input: String,
    ) -> Vec<TaskPayload> {
        // Create a fuzzy matcher with default settings, ignoring case
        let matcher = SkimMatcherV2::default().ignore_case();

        // If search input is empty, return no tasks
        if search_input.trim().is_empty() {
            return vec![];
        }

        // Pre-filter search to reduce fuzzy match calls
        let search_input_lower = search_input.to_lowercase();
        let match_results = search
            .into_iter()
            .filter(|s| s.as_ref().to_lowercase().contains(&search_input_lower))
            .filter_map(|s| {
                let s_str = s.as_ref();
                // Use fuzzy matching, with fallback for single-letter inputs
                matcher.fuzzy_indices(s_str, &search_input).map(|(score, indices)| {
                    let adjusted_score = if search_input.len() == 1 {
                        score.max(1) // Ensure single-letter matches have a positive score
                    } else {
                        score
                    };
                    (s, adjusted_score, indices)
                })
            })
            .collect::<Vec<_>>();

        // Create a map of task IDs to tasks for O(1) lookups
        let task_map: std::collections::HashMap<_, _> = self
            .iter()
            .map(|task| (task.id.key_string(), task))
            .collect();

        // Collect tasks with their best match scores
        let mut task_scores = vec![];
        let mut seen_ids = std::collections::HashSet::with_capacity(self.len());

        for (output, input_score, _) in match_results {
            let output_str = output.as_ref();
            for task in self.iter() {
                let task_id = task.id.key_string().clone();
                if seen_ids.contains(&task_id) {
                    continue;
                }
                let task_name = task.task_name.as_str();
                // Compute fuzzy match score for task_name only
                if let Some((task_score, _)) = matcher.fuzzy_indices(task_name, output_str) {
                    let combined_score = task_score.max(input_score);
                    task_scores.push((task_id.clone(), combined_score));
                    seen_ids.insert(task_id);
                }
            }
        }

        // Sort tasks by score in descending order
        task_scores.sort_by_key(|&(_, score)| Reverse(score));

        // Collect unique tasks, avoiding clones
        task_scores
            .into_iter()
            .filter_map(|(task_id, _)| task_map.get(&task_id).map(|task| *task))
            .cloned()
            .collect::<Vec<TaskPayload>>()
    }
}
