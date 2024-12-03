use crate::{FilterClients, FilterTasks};
use database::schema::{ConnectedClient, Priority, Status, Store, TaskPayload, User};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use std::cmp::Reverse;

impl FilterTasks for Vec<TaskPayload> {
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.assignee == assignee.id)
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

    fn filter_by_date(&self, date: &String) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| task.due_date >= *date)
            .cloned()
            .collect()
    }

    fn filter_by_store(&self, assignee: &User, store: &Store) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| {
                assignee.store == *store && task.assignee.key().to_string() == assignee.id.key().to_string()
            })
            .cloned()
            .collect()
    }

    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        search: T,
        search_input: String,
    ) -> Vec<TaskPayload> {
        // Create a fuzzy matcher with default settings, ignoring case.
        let matcher = SkimMatcherV2::default().ignore_case();

        // Initialize a vector to hold the match results.
        let mut match_results = search
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
                .filter(|task| {
                    task.task_name.contains(output.as_ref())
                        || task
                            .service_number
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
