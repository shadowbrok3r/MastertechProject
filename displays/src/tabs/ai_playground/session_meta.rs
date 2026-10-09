//! Customer names and plan progress for the agent session rows, fetched in the background.

use std::collections::{HashMap, HashSet};

use crossbeam::channel::{Receiver, Sender};
use database::schema::{AgentThread, Plan, RecordId, RecordIdExt};
use eframe::egui::Context;

use super::session_list::RowExtra;
use crate::ui_tools::chat_bubble;
use crate::{PlatformSpawner, Spawner};

/// Shortest gap between two background fetches.
const FETCH_GAP: web_time::Duration = web_time::Duration::from_secs(3);

/// Latest plans by thread, and customer names by service number.
type Found = (Vec<(RecordId, Plan)>, Vec<(String, String)>);

/// What one background fetch asked for and what it found.
struct Fetched {
    threads: Vec<RecordId>,
    numbers: Vec<String>,
    found: Result<Found, String>,
}

/// Customer names by service number and the latest plan of each listed session.
pub(super) struct SessionMeta {
    customers: HashMap<String, String>,
    asked_numbers: HashSet<String>,
    plans: HashMap<String, Plan>,
    /// `last_seq` of each thread when its plan was last asked for.
    seen_seq: HashMap<String, i64>,
    in_flight: bool,
    last_fetch: Option<web_time::Instant>,
    tx: Sender<Fetched>,
    rx: Receiver<Fetched>,
}

impl Default for SessionMeta {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            customers: HashMap::new(),
            asked_numbers: HashSet::new(),
            plans: HashMap::new(),
            seen_seq: HashMap::new(),
            in_flight: false,
            last_fetch: None,
            tx,
            rx,
        }
    }
}

impl SessionMeta {
    /// The customer named on `thread`'s service order, once fetched.
    pub(super) fn customer(&self, thread: &AgentThread) -> Option<&str> {
        self.customers.get(thread.service_number.as_deref()?.trim()).map(String::as_str)
    }

    /// The latest plan of `thread`, once fetched.
    pub(super) fn plan(&self, thread: &AgentThread) -> Option<&Plan> {
        self.plans.get(&thread.id.key_string())
    }

    /// The customer and plan progress of each of `threads`, keyed by thread.
    pub(super) fn extras(&self, threads: &[&AgentThread]) -> HashMap<String, RowExtra> {
        threads
            .iter()
            .map(|t| {
                let plan = self.plan(t).map(|p| (p.share(), format!("Plan: {}", chat_bubble::plan_summary(p))));
                (t.id.key_string(), RowExtra { customer: self.customer(t).map(str::to_string), plan })
            })
            .collect()
    }

    /// Applies finished fetches; a failed one is asked for again on a later frame.
    pub(super) fn receive(&mut self) {
        while let Ok(fetched) = self.rx.try_recv() {
            self.in_flight = false;
            match fetched.found {
                Ok((plans, names)) => {
                    for id in &fetched.threads {
                        self.plans.remove(&id.key_string());
                    }
                    self.plans.extend(plans.into_iter().map(|(id, plan)| (id.key_string(), plan)));
                    self.customers.extend(names);
                }
                Err(e) => {
                    log::warn!("session list details could not be read: {e}");
                    for id in &fetched.threads {
                        self.seen_seq.remove(&id.key_string());
                    }
                    for number in &fetched.numbers {
                        self.asked_numbers.remove(number);
                    }
                }
            }
        }
    }

    /// Starts a fetch for threads whose last event changed and service numbers not looked up yet; one at a time.
    pub(super) fn request(&mut self, threads: &[&AgentThread], ctx: &Context) {
        if self.in_flight || self.last_fetch.is_some_and(|at| at.elapsed() < FETCH_GAP) {
            return;
        }
        let (stale, numbers) = self.wanted(threads);
        if stale.is_empty() && numbers.is_empty() {
            return;
        }
        for thread in threads {
            if stale.contains(&thread.id) {
                self.seen_seq.insert(thread.id.key_string(), thread.last_seq.unwrap_or(-1));
            }
        }
        self.asked_numbers.extend(numbers.iter().cloned());
        self.in_flight = true;
        self.last_fetch = Some(web_time::Instant::now());
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        PlatformSpawner::spawn(async move {
            let plans = database::schema::agent_plan::latest_plans(&stale).await;
            let names = database::schema::agent_thread::customer_names(&numbers).await;
            let found = match (plans, names) {
                (Ok(plans), Ok(names)) => Ok((plans, names)),
                (Err(e), _) | (_, Err(e)) => Err(e.to_string()),
            };
            let _ = tx.send(Fetched { threads: stale, numbers, found });
            ctx.request_repaint();
        });
    }

    /// Threads whose `last_seq` moved since their plan was read, and service numbers without a lookup.
    fn wanted(&self, threads: &[&AgentThread]) -> (Vec<RecordId>, Vec<String>) {
        let stale = threads
            .iter()
            .filter(|t| self.seen_seq.get(&t.id.key_string()) != Some(&t.last_seq.unwrap_or(-1)))
            .map(|t| t.id.clone())
            .collect();
        let numbers: HashSet<String> = threads
            .iter()
            .filter_map(|t| t.service_number.as_deref().map(str::trim))
            .filter(|sn| !sn.is_empty() && !self.asked_numbers.contains(*sn))
            .map(str::to_string)
            .collect();
        (stale, numbers.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::ai_playground::session_list::tests::thread;

    #[test]
    fn only_changed_threads_and_new_service_numbers_are_asked_for() {
        let mut meta = SessionMeta::default();
        let mut a = thread("a", "running", "PC-A:1", None);
        a.service_number = Some("2141021".into());
        a.last_seq = Some(10);
        let b = thread("b", "idle", "PC-B:2", None);
        let (stale, numbers) = meta.wanted(&[&a, &b]);
        assert_eq!(stale.len(), 2);
        assert_eq!(numbers, vec!["2141021".to_string()]);

        meta.seen_seq.insert("a".into(), 10);
        meta.seen_seq.insert("b".into(), -1);
        meta.asked_numbers.insert("2141021".into());
        assert_eq!(meta.wanted(&[&a, &b]), (Vec::new(), Vec::new()));
        a.last_seq = Some(11);
        assert_eq!(meta.wanted(&[&a, &b]).0, vec![a.id.clone()]);
    }

    #[test]
    fn a_fetch_replaces_plans_and_a_failure_is_asked_again() {
        let mut meta = SessionMeta::default();
        let a = thread("a", "running", "PC-A:1", None);
        let plan = Plan::from_value(&serde_json::json!({ "plan": [{ "step": "Scans", "status": "completed" }] }))
            .expect("plan");
        meta.plans.insert("a".into(), plan.clone());
        meta.tx
            .send(Fetched { threads: vec![a.id.clone()], numbers: Vec::new(), found: Ok((Vec::new(), Vec::new())) })
            .unwrap();
        meta.receive();
        assert!(meta.plan(&a).is_none(), "a thread whose plan is gone loses it");

        let mut b = thread("b", "idle", "PC-B:2", None);
        b.service_number = Some("2141021".into());
        meta.tx
            .send(Fetched {
                threads: vec![b.id.clone()],
                numbers: vec!["2141021".into()],
                found: Ok((vec![(b.id.clone(), plan)], vec![("2141021".into(), "Martin Empey".into())])),
            })
            .unwrap();
        meta.receive();
        let extra = meta.extras(&[&b]).remove("b").expect("row extra");
        assert_eq!(extra.customer.as_deref(), Some("Martin Empey"));
        assert_eq!(extra.plan, Some((1.0, "Plan: 1 of 1 done".to_string())));

        meta.seen_seq.insert("a".into(), 3);
        meta.asked_numbers.insert("2141021".into());
        meta.in_flight = true;
        meta.tx
            .send(Fetched {
                threads: vec![a.id.clone()],
                numbers: vec!["2141021".into()],
                found: Err("offline".into()),
            })
            .unwrap();
        meta.receive();
        assert!(!meta.in_flight);
        assert!(meta.seen_seq.is_empty() && meta.asked_numbers.is_empty());
    }
}
