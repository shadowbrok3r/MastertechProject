use crate::{FilterClients, FilterTasks};
use database::schema::{ConnectedClient, Priority, Status, Store, TaskPayload, User};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use chrono::{DateTime, Utc};
use std::cmp::Reverse;

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
                assignee.get_store() == *store && task.assignee.key().to_string() == assignee.get_id().key().to_string()
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
            .map(|task| (task.id.key().to_string(), task))
            .collect();

        // Collect tasks with their best match scores
        let mut task_scores = vec![];
        let mut seen_ids = std::collections::HashSet::with_capacity(self.len());

        for (output, input_score, _) in match_results {
            let output_str = output.as_ref();
            for task in self.iter() {
                let task_id = task.id.key().to_string().clone();
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

impl FilterClients for Vec<ConnectedClient> {
    fn filter_by_client<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        name: T,
        search_input: String,
    ) -> Vec<ConnectedClient> {
        // Create a fuzzy matcher with default settings, ignoring case.
        let matcher = SkimMatcherV2::default().ignore_case();

        // Initialize a vector to hold the match results.
        let mut match_results = name
            // Convert the input iterator into an iterator of the items.
            .into_iter()
            // Filter and map the items based on the fuzzy match score.
            .filter_map(|s| {
                // Calculate the fuzzy match score and the matched indices.
                let score = matcher.fuzzy_indices(s.as_ref(), search_input.as_str());
                // If a match is found, map it to a tuple of (item, score, indices).
                score.map(|(score, indices)| (s, score, indices))
            })
            // Collect the filtered and mapped results into a vector.
            .collect::<Vec<_>>();

        // Sort the match results by score in descending order (higher scores first).
        match_results.sort_by_key(|k| Reverse(k.1));

        for (_i, (output, _, _match_indices)) in match_results.iter().take(6).enumerate() {
            return self
                .into_iter()
                .filter(|client| {
                    client.connection_string.contains(output.as_ref())
                        || client
                            .friendly_name
                            .clone()
                            .unwrap_or_default()
                            .contains(output.as_ref())
                })
                .cloned()
                .collect();
        }
        self.to_vec()
    }
}
