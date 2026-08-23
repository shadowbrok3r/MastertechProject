//! Operator-facing task search: parses a raw query string into intents
//! (customer name, phone number, service number, assignee, completion scope)
//! and runs them against the `task` table, joining through
//! `service_ticket.customer` for fields the client never loads.

use crate::{db, schema::LiveTaskPayload};

/// Completion filter derived from the query text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskScope {
    /// Both open and completed tasks.
    #[default]
    Any,
    Open,
    Completed,
}

/// Shortest digit run treated as an identifier rather than free text.
/// Service numbers are 7 digits, phone numbers 10.
const MIN_IDENTIFIER_DIGITS: usize = 7;

/// How much meaning to read into the query text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QueryMode {
    /// Words are just words. Every token must appear somewhere in the task's
    /// text or its customer's name; nothing is treated as an operator.
    #[default]
    Literal,
    /// Recognizes "tasks for <name>", "assigned to <name>", "<name>'s tasks",
    /// and open/completed scope words, and drops filler words.
    Semantic,
}

/// A parsed search query. Free-text terms are ANDed; identifier
/// interpretations (phone, service number) are ORed with each other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskQuery {
    pub raw: String,
    /// Terms matched across task name, description, service number, customer name.
    pub terms: Vec<String>,
    /// Terms restricted to the customer, from an explicit "for <name>".
    pub customer_terms: Vec<String>,
    /// Digit-only phone fragment.
    pub phone_digits: Option<String>,
    /// Digit run that could be a service number.
    pub service_number: Option<String>,
    /// Name fragment from "assigned to <name>".
    pub assignee_term: Option<String>,
    pub scope: TaskScope,
}

/// Scope words and the completion filter each implies, longest first so
/// "completed" wins over "complete".
const SCOPE_WORDS: &[(&str, TaskScope)] = &[
    ("completed", TaskScope::Completed),
    ("complete", TaskScope::Completed),
    ("finished", TaskScope::Completed),
    ("closed", TaskScope::Completed),
    ("done", TaskScope::Completed),
    ("open", TaskScope::Open),
    ("active", TaskScope::Open),
    ("incomplete", TaskScope::Open),
    ("outstanding", TaskScope::Open),
];

/// Words dropped from free-text terms; they carry intent, not content.
const NOISE: &[&str] = &[
    "task", "tasks", "the", "a", "an", "of", "on", "with", "show", "me", "find", "all", "any",
];

/// Phrases that introduce a customer name.
const CUSTOMER_LEADS: &[&str] = &[
    "tasks for", "task for", "customer", "cust", "client", "for",
];

/// Phrases that introduce an assignee name. Deliberately narrow: "tech" and
/// "by" occur too often inside real task names to treat as operators.
const ASSIGNEE_LEADS: &[&str] = &["assigned to", "assignee", "belonging to"];

impl TaskQuery {
    /// Every token must match, with no words treated as operators.
    pub fn literal(raw: &str) -> Self {
        Self::parse_with(raw, QueryMode::Literal)
    }

    /// Full intent extraction. See [`QueryMode::Semantic`].
    pub fn parse(raw: &str) -> Self {
        Self::parse_with(raw, QueryMode::Semantic)
    }

    /// Splits `raw` into intents. Never fails: anything unrecognized stays
    /// free text, so the query degrades to a plain name search.
    pub fn parse_with(raw: &str, mode: QueryMode) -> Self {
        let semantic = mode == QueryMode::Semantic;
        let mut work = raw.trim().to_lowercase();
        let mut scope = TaskScope::Any;

        // Scope keywords are removed so they never survive as search terms.
        for (needle, found) in if semantic {
            SCOPE_WORDS
        } else {
            &[] as &[(&str, TaskScope)]
        } {
            if let Some(stripped) = remove_word(&work, needle) {
                work = stripped;
                scope = *found;
                break;
            }
        }

        // "<name>'s tasks" reads as an assignee, not a customer.
        let mut assignee_term = None;
        if semantic {
        if let Some(idx) = work.find("'s task") {
            let (owner, rest) = work.split_at(idx);
            let owner = owner.trim().to_string();
            if !owner.is_empty() {
                assignee_term = Some(owner);
                work = rest.trim_start_matches(|c: char| c != ' ').trim().to_string();
            }
        }
        }

        if semantic && assignee_term.is_none() {
            if let Some((lead_end, _)) = find_lead(&work, ASSIGNEE_LEADS) {
                let tail = work[lead_end..].trim().to_string();
                if !tail.is_empty() {
                    assignee_term = Some(tail);
                    work = work[..find_lead(&work, ASSIGNEE_LEADS).map(|(_, s)| s).unwrap_or(0)]
                        .trim()
                        .to_string();
                }
            }
        }

        // An explicit customer lead narrows everything after it to the customer.
        let mut customer_terms = Vec::new();
        if semantic {
            if let Some((lead_end, lead_start)) = find_lead(&work, CUSTOMER_LEADS) {
                let tail = work[lead_end..].trim().to_string();
                if !tail.is_empty() {
                    customer_terms = tokenize(&tail, semantic);
                    work = work[..lead_start].trim().to_string();
                }
            }
        }

        // Digit runs are ambiguous between service number and phone; keep both
        // readings and let the query OR them.
        let digits: String = work.chars().filter(|c| c.is_ascii_digit()).collect();
        let non_digit_chars = work
            .chars()
            .filter(|c| !c.is_ascii_digit() && !c.is_whitespace() && !"-().+".contains(*c))
            .count();
        let mut phone_digits = None;
        let mut service_number = None;
        if non_digit_chars == 0 && digits.len() >= MIN_IDENTIFIER_DIGITS {
            phone_digits = Some(digits.clone());
            service_number = Some(digits.clone());
            work.clear();
        }

        Self {
            raw: raw.trim().to_string(),
            terms: tokenize(&work, semantic),
            customer_terms,
            phone_digits,
            service_number,
            assignee_term,
            scope,
        }
    }

    /// True when nothing survived parsing, so there is nothing to search for.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
            && self.customer_terms.is_empty()
            && self.phone_digits.is_none()
            && self.service_number.is_none()
            && self.assignee_term.is_none()
    }

    /// True when answering this query needs fields absent from
    /// `LiveTaskPayload` (customer name, phone), so the DB must be consulted.
    pub fn needs_database(&self) -> bool {
        self.phone_digits.is_some() || !self.customer_terms.is_empty()
    }

    /// Matches against the in-memory index. `assignee_name` is the resolved
    /// display name of the task's assignee, when known.
    ///
    /// Customer name and phone are unavailable locally; queries that depend on
    /// them fall through to `search_tasks`. Task names embed the customer name
    /// by convention, so a customer term still matches here most of the time.
    pub fn matches_local(&self, task: &LiveTaskPayload, assignee_name: Option<&str>) -> bool {
        match self.scope {
            TaskScope::Open if task.completed => return false,
            TaskScope::Completed if !task.completed => return false,
            _ => {}
        }

        if let Some(ref who) = self.assignee_term {
            let name = assignee_name.unwrap_or_default().to_lowercase();
            if !tokenize(who, true).iter().all(|t| name.contains(t.as_str())) {
                return false;
            }
        }

        let haystack = local_haystack(task);

        if !self.terms.iter().all(|t| haystack.contains(t.as_str())) {
            return false;
        }
        if !self.customer_terms.is_empty()
            && !self.customer_terms.iter().all(|t| haystack.contains(t.as_str()))
        {
            return false;
        }

        // Identifier readings are alternatives, so one hit is enough.
        if self.phone_digits.is_some() || self.service_number.is_some() {
            let svc_hit = self
                .service_number
                .as_deref()
                .is_some_and(|sn| haystack.contains(sn));
            if !svc_hit {
                return false;
            }
        }

        true
    }

    /// Runs the query against the DB, scoped to `store`. Returns at most
    /// `limit` tasks, most recently created first.
    pub async fn search(
        &self,
        store: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<LiveTaskPayload>, anyhow::Error> {
        if self.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_of: Vec<String> = Vec::new();
        let mut any_of: Vec<String> = Vec::new();
        let mut binds: Vec<(String, String)> = Vec::new();

        for (i, term) in self.terms.iter().enumerate() {
            let p = format!("t{i}");
            all_of.push(format!(
                "(string::lowercase(task_name) CONTAINS ${p} \
                 OR string::lowercase(task_description ?? '') CONTAINS ${p} \
                 OR string::lowercase(service_number ?? '') CONTAINS ${p} \
                 OR string::lowercase(service_ticket.customer.name ?? '') CONTAINS ${p})"
            ));
            binds.push((p, term.clone()));
        }

        for (i, term) in self.customer_terms.iter().enumerate() {
            let p = format!("c{i}");
            all_of.push(format!(
                "(string::lowercase(service_ticket.customer.name ?? '') CONTAINS ${p} \
                 OR string::lowercase(task_name) CONTAINS ${p})"
            ));
            binds.push((p, term.clone()));
        }

        if let Some(ref who) = self.assignee_term {
            for (i, term) in tokenize(who, true).into_iter().enumerate() {
                let p = format!("a{i}");
                all_of.push(format!(
                    "(string::lowercase(assignee.name ?? '') CONTAINS ${p} \
                     OR string::lowercase(assignee.email ?? '') CONTAINS ${p})"
                ));
                binds.push((p, term));
            }
        }

        if let Some(ref digits) = self.phone_digits {
            any_of.push(format!(
                "({p1} CONTAINS $phone OR {p2} CONTAINS $phone)",
                p1 = digits_only("service_ticket.customer.phone_number"),
                p2 = digits_only("service_ticket.customer.phone_number_2"),
            ));
            binds.push(("phone".to_string(), digits.clone()));
        }

        if let Some(ref sn) = self.service_number {
            any_of.push("string::lowercase(service_number ?? '') CONTAINS $svc".to_string());
            binds.push(("svc".to_string(), sn.clone()));
        }

        match self.scope {
            TaskScope::Open => all_of.push("completed == false".to_string()),
            TaskScope::Completed => all_of.push("completed == true".to_string()),
            TaskScope::Any => {}
        }

        all_of.push("assignee.store == $store".to_string());

        if !any_of.is_empty() {
            all_of.push(format!("({})", any_of.join(" OR ")));
        }

        // completed_at is absent on rows predating the field; created_at keeps
        // the ordering total.
        let query = format!(
            "SELECT * FROM task WHERE {} ORDER BY created_at DESC LIMIT $limit",
            all_of.join(" AND ")
        );

        let conn = db();
        let mut request = conn
            .query(query)
            .bind(("store", store.to_string()))
            .bind(("limit", limit as i64));
        for (name, value) in binds {
            request = request.bind((name, value));
        }

        Ok(request.await?.take(0)?)
    }
}

/// SurrealQL expression stripping separators from a phone field so a typed
/// run of digits matches the stored dash-formatted value.
fn digits_only(field: &str) -> String {
    format!(
        "string::replace(string::replace(string::replace(string::replace(string::replace({field} ?? '', '-', ''), ' ', ''), '(', ''), ')', ''), '.', '')"
    )
}

/// Everything on a task that a local match may look at, lowercased.
fn local_haystack(task: &LiveTaskPayload) -> String {
    let mut s = task.task_name.to_lowercase();
    s.push(' ');
    s.push_str(&task.task_description.to_lowercase());
    s.push(' ');
    s.push_str(&task.service_number.clone().unwrap_or_default().to_lowercase());
    s
}

/// Lowercased words split on punctuation. Filler words are only dropped in
/// semantic mode; a literal search takes every word the operator typed.
fn tokenize(s: &str, drop_noise: bool) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| !drop_noise || !NOISE.contains(&w.as_str()))
        .collect()
}

/// Removes `word` when it appears as a whole word. Returns `None` if absent.
fn remove_word(haystack: &str, word: &str) -> Option<String> {
    let start = haystack.find(word)?;
    let end = start + word.len();
    let before_ok = start == 0 || !haystack[..start].ends_with(|c: char| c.is_alphanumeric());
    let after_ok = end == haystack.len() || !haystack[end..].starts_with(|c: char| c.is_alphanumeric());
    if !before_ok || !after_ok {
        return None;
    }
    let mut out = String::with_capacity(haystack.len() - word.len());
    out.push_str(haystack[..start].trim_end());
    if !out.is_empty() && end < haystack.len() {
        out.push(' ');
    }
    out.push_str(haystack[end..].trim_start());
    Some(out.trim().to_string())
}

/// Byte offsets `(after_lead, at_lead)` of the earliest matching lead phrase.
/// Longest phrases are tried first so "tasks for" beats "for".
fn find_lead(haystack: &str, leads: &[&str]) -> Option<(usize, usize)> {
    let mut sorted: Vec<&&str> = leads.iter().collect();
    sorted.sort_by_key(|l| std::cmp::Reverse(l.len()));
    for lead in sorted {
        let mut from = 0;
        while let Some(rel) = haystack[from..].find(*lead) {
            let start = from + rel;
            let end = start + lead.len();
            let before_ok =
                start == 0 || !haystack[..start].ends_with(|c: char| c.is_alphanumeric());
            let after_ok = end == haystack.len()
                || !haystack[end..].starts_with(|c: char| c.is_alphanumeric());
            if before_ok && after_ok {
                return Some((end, start));
            }
            from = end;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_name_becomes_terms() {
        let q = TaskQuery::parse("Ross LaRue");
        assert_eq!(q.terms, vec!["ross", "larue"]);
        assert!(q.customer_terms.is_empty());
        assert_eq!(q.scope, TaskScope::Any);
        assert!(!q.needs_database());
    }

    #[test]
    fn tasks_for_name_targets_customer() {
        let q = TaskQuery::parse("Tasks for Ross LaRue");
        assert_eq!(q.customer_terms, vec!["ross", "larue"]);
        assert!(q.terms.is_empty());
        assert!(q.needs_database());
    }

    #[test]
    fn phone_number_parses_to_digits() {
        let q = TaskQuery::parse("801-510-1399");
        assert_eq!(q.phone_digits.as_deref(), Some("8015101399"));
        assert_eq!(q.service_number.as_deref(), Some("8015101399"));
        assert!(q.terms.is_empty());
        assert!(q.needs_database());
    }

    #[test]
    fn spaced_and_parenthesized_phone_parses() {
        let q = TaskQuery::parse("(801) 510 1399");
        assert_eq!(q.phone_digits.as_deref(), Some("8015101399"));
    }

    #[test]
    fn service_number_parses_as_identifier() {
        let q = TaskQuery::parse("2153583");
        assert_eq!(q.service_number.as_deref(), Some("2153583"));
    }

    #[test]
    fn short_digit_run_stays_free_text() {
        let q = TaskQuery::parse("2153");
        assert_eq!(q.terms, vec!["2153"]);
        assert!(q.service_number.is_none());
    }

    #[test]
    fn completed_keyword_sets_scope() {
        let q = TaskQuery::parse("completed ross");
        assert_eq!(q.scope, TaskScope::Completed);
        assert_eq!(q.terms, vec!["ross"]);
    }

    #[test]
    fn open_keyword_sets_scope() {
        let q = TaskQuery::parse("open tasks for smith");
        assert_eq!(q.scope, TaskScope::Open);
        assert_eq!(q.customer_terms, vec!["smith"]);
    }

    #[test]
    fn assigned_to_targets_assignee() {
        let q = TaskQuery::parse("assigned to logan");
        assert_eq!(q.assignee_term.as_deref(), Some("logan"));
        assert!(q.terms.is_empty());
    }

    #[test]
    fn possessive_targets_assignee() {
        let q = TaskQuery::parse("logan's tasks");
        assert_eq!(q.assignee_term.as_deref(), Some("logan"));
    }

    #[test]
    fn noise_words_are_dropped() {
        let q = TaskQuery::parse("show me all tasks smith");
        assert_eq!(q.terms, vec!["smith"]);
    }

    #[test]
    fn a_word_containing_a_keyword_is_not_stripped() {
        // "openshaw" must not be read as the "open" scope keyword.
        let q = TaskQuery::parse("openshaw");
        assert_eq!(q.scope, TaskScope::Any);
        assert_eq!(q.terms, vec!["openshaw"]);
    }

    #[test]
    fn tech_and_by_are_not_assignee_operators() {
        // Both words are common inside real task names.
        let q = TaskQuery::parse("tech bench 3");
        assert!(q.assignee_term.is_none());
        assert_eq!(q.terms, vec!["tech", "bench", "3"]);
    }

    #[test]
    fn phone_only_matches_locally_via_service_number() {
        let mut task = LiveTaskPayload::default();
        task.task_name = "Ross LaRue - 2134740".to_string();
        task.service_number = Some("2134740".to_string());
        // A phone number lives on the customer, so only the DB can answer it.
        let q = TaskQuery::parse("801-510-1399");
        assert!(!q.matches_local(&task, None));
        assert!(q.needs_database());
    }

    #[test]
    fn literal_mode_treats_operators_as_words() {
        let q = TaskQuery::literal("tasks for josh");
        assert!(q.customer_terms.is_empty(), "no lead extraction");
        assert!(q.assignee_term.is_none());
        // Filler words survive: the operator typed them, so they must match.
        assert_eq!(q.terms, vec!["tasks", "for", "josh"]);
    }

    #[test]
    fn literal_mode_ignores_scope_keywords() {
        let q = TaskQuery::literal("completed smith");
        assert_eq!(q.scope, TaskScope::Any);
        assert_eq!(q.terms, vec!["completed", "smith"]);
    }

    #[test]
    fn literal_mode_ignores_the_possessive_form() {
        let q = TaskQuery::literal("josh's tasks");
        assert!(q.assignee_term.is_none());
        assert_eq!(q.terms, vec!["josh", "s", "tasks"]);
    }

    #[test]
    fn literal_mode_still_recognizes_a_phone_number() {
        // Field normalization, not language: both modes answer a phone number.
        let q = TaskQuery::literal("801-510-1399");
        assert_eq!(q.phone_digits.as_deref(), Some("8015101399"));
        assert!(q.needs_database());
    }

    #[test]
    fn literal_and_semantic_agree_on_a_plain_name() {
        assert_eq!(
            TaskQuery::literal("ross larue").terms,
            TaskQuery::parse("ross larue").terms
        );
    }

    #[test]
    fn default_mode_is_literal() {
        assert_eq!(QueryMode::default(), QueryMode::Literal);
    }

    #[test]
    fn empty_query_is_empty() {
        assert!(TaskQuery::parse("   ").is_empty());
        assert!(TaskQuery::parse("tasks").is_empty());
    }

    #[test]
    fn local_match_respects_scope_and_terms() {
        let mut task = LiveTaskPayload::default();
        task.task_name = "Ross LaRue - 2134740".to_string();
        task.service_number = Some("2134740".to_string());
        task.completed = true;

        assert!(TaskQuery::parse("ross larue").matches_local(&task, None));
        assert!(TaskQuery::parse("completed ross").matches_local(&task, None));
        assert!(!TaskQuery::parse("open ross").matches_local(&task, None));
        assert!(TaskQuery::parse("2134740").matches_local(&task, None));
        assert!(!TaskQuery::parse("9999999").matches_local(&task, None));
    }

    #[test]
    fn local_match_finds_customer_via_task_name() {
        let mut task = LiveTaskPayload::default();
        task.task_name = "Ross LaRue - 2134740".to_string();
        assert!(TaskQuery::parse("tasks for ross larue").matches_local(&task, None));
    }

    #[test]
    fn local_match_filters_by_assignee_name() {
        let mut task = LiveTaskPayload::default();
        task.task_name = "Ross LaRue - 2134740".to_string();
        assert!(TaskQuery::parse("assigned to logan").matches_local(&task, Some("Logan Lees")));
        assert!(!TaskQuery::parse("assigned to logan").matches_local(&task, Some("Sam Ferg")));
    }
}
