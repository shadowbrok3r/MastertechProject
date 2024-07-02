use std::cmp::Reverse;

use database::schema::{Priority, User, Status, TaskPayload};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use super::FilterTasks;


impl FilterTasks for Vec<TaskPayload>{
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload> {
        self.into_iter()
            .filter(|task| 
                // let mut res = false;
                // for user in assignees{
                    task.everest_initials == assignee.everest_initials
                // }
                // res
            )
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

    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>
    (&self, search: T, search_input: String) -> Vec<TaskPayload> 
    {
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

        for (i, (output, _, _match_indices)) in
            match_results.iter().take(6).enumerate()
        {
            return self.into_iter()
                .filter(
                    |task| 
                        task.task_name.contains(output.as_ref()) 
                        || task.service_number.clone().unwrap_or_default().contains(output.as_ref())
                )
                .cloned()
                .collect();
        }
        self.to_vec()
    }
}