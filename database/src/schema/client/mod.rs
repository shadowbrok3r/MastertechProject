use crate::schema::{SortDirection, Sortable, CONNECTED_CLIENT_TABLE};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use structdiff::{Difference, StructDiff};
use std::cmp::Reverse;

use super::{random_record_id, Datetime, RecordId, SurrealValue};

#[derive(serde::Serialize, Debug, Clone, serde::Deserialize, PartialEq, Difference, SurrealValue)]
pub struct ConnectedClient {
    pub id: RecordId,
    pub assigned_user: Option<RecordId>,
    pub client_hash: String,
    pub connection_string: String,
    pub command_history: Option<Vec<String>>,
    pub connected: bool,
    pub friendly_name: Option<String>,
    pub customer: Option<RecordId>,
    pub last_update: Option<Datetime>,
    pub created_at: Option<Datetime>,
    pub computer:  Option<RecordId>
}

impl Default for ConnectedClient {
    fn default() -> Self {
        Self {
            id: random_record_id(CONNECTED_CLIENT_TABLE),
            assigned_user: Default::default(),
            client_hash: Default::default(),
            connection_string: Default::default(),
            command_history: Default::default(),
            connected: Default::default(),
            friendly_name: Default::default(),
            customer: Default::default(),
            last_update: Default::default(),
            created_at: Default::default(),
            computer: Default::default(),
        }
    }
}

impl Sortable<ConnectedClient> for Vec<ConnectedClient> {
    fn default_sort(&mut self,  sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by_date(sort_direction)
    }

    fn sort_by_date(&mut self, sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by(|a: &ConnectedClient, b: &ConnectedClient| {
            let date_a = &a.last_update.as_ref().cloned().unwrap_or_default();
            let date_b = &b.last_update.as_ref().cloned().unwrap_or_default();
            
            let ordering = date_b.cmp(&date_a);
            
            match sort_direction {
                SortDirection::Asc => ordering,               // Use default ordering for ascending
                SortDirection::Desc => ordering.reverse(),    // Reverse ordering for descending
            }
        });
    
        self
    }

    fn sort_by_name(&mut self, sort_direction: SortDirection) -> &mut Vec<ConnectedClient> {
        self.sort_by(|a, b| {
            let name_a = &a.connection_string.to_lowercase();
            let name_b = &b.connection_string.to_lowercase();
            
            let ordering = name_a.cmp(name_b);
    
            match sort_direction {
                SortDirection::Asc => ordering,              // Default alphabetical ordering (A-Z)
                SortDirection::Desc => ordering.reverse(),   // Reverse alphabetical ordering (Z-A)
            }
        });
    
        self
    }
}

pub trait FilterClients {
    fn filter_by_client<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(
        &self,
        name: T,
        search_input: String,
    ) -> Vec<ConnectedClient>;
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
