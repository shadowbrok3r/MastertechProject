//! The agent's `update_plan` checklist, stored as an `other` agent_event whose item type is `planUpdate`.

use serde_json::{Value, json};

/// `item.type` of a stored plan update.
pub const PLAN_ITEM_TYPE: &str = "planUpdate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

impl PlanStatus {
    fn parse(raw: &str) -> Self {
        match raw {
            "completed" => Self::Completed,
            "inProgress" | "in_progress" => Self::InProgress,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    pub step: String,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
}

impl Plan {
    /// The plan in a `turn/plan/updated` notification or a stored plan item; `None` without steps.
    pub fn from_value(value: &Value) -> Option<Self> {
        let steps: Vec<PlanStep> = value
            .get("plan")?
            .as_array()?
            .iter()
            .filter_map(|s| {
                let step = s.get("step")?.as_str()?.trim();
                let status = s.get("status").and_then(Value::as_str).unwrap_or("pending");
                (!step.is_empty()).then(|| PlanStep { step: step.to_string(), status: PlanStatus::parse(status) })
            })
            .collect();
        if steps.is_empty() {
            return None;
        }
        let explanation = value
            .get("explanation")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .map(str::to_string);
        Some(Self { explanation, steps })
    }

    /// The `item` the broker stores for this plan.
    pub fn to_item(&self) -> Value {
        let plan: Vec<Value> = self
            .steps
            .iter()
            .map(|s| {
                let status = match s.status {
                    PlanStatus::Pending => "pending",
                    PlanStatus::InProgress => "inProgress",
                    PlanStatus::Completed => "completed",
                };
                json!({ "step": s.step, "status": status })
            })
            .collect();
        json!({ "type": PLAN_ITEM_TYPE, "explanation": self.explanation, "plan": plan })
    }

    pub fn done(&self) -> usize {
        self.steps.iter().filter(|s| s.status == PlanStatus::Completed).count()
    }

    pub fn complete(&self) -> bool {
        self.done() == self.steps.len()
    }

    pub fn completed_steps(&self) -> Vec<PlanStep> {
        self.steps
            .iter()
            .filter(|s| s.status == PlanStatus::Completed)
            .cloned()
            .collect()
    }

    /// This plan led by the `carried` steps it does not already name.
    pub fn with_carried(mut self, carried: &[PlanStep]) -> Self {
        let named: Vec<String> = self.steps.iter().map(|s| step_key(&s.step)).collect();
        let mut steps: Vec<PlanStep> = carried
            .iter()
            .filter(|c| !named.contains(&step_key(&c.step)))
            .cloned()
            .collect();
        steps.append(&mut self.steps);
        self.steps = steps;
        self
    }

    /// The step being worked on: the first in progress, else the first pending.
    pub fn current(&self) -> Option<&PlanStep> {
        self.steps
            .iter()
            .find(|s| s.status == PlanStatus::InProgress)
            .or_else(|| self.steps.iter().find(|s| s.status == PlanStatus::Pending))
    }

    /// Plain markdown checklist with a progress header.
    pub fn text(&self) -> String {
        let mut out = format!("Plan ({} of {} done)", self.done(), self.steps.len());
        if let Some(why) = &self.explanation {
            out.push('\n');
            out.push_str(why);
        }
        for s in &self.steps {
            let line = match s.status {
                PlanStatus::Completed => format!("\n- [x] {}", s.step),
                PlanStatus::InProgress => format!("\n- [ ] {} (in progress)", s.step),
                PlanStatus::Pending => format!("\n- [ ] {}", s.step),
            };
            out.push_str(&line);
        }
        out
    }
}

/// Lowercase alphanumeric words of a step, for matching rewordings that differ only in punctuation or case.
fn step_key(step: &str) -> String {
    step.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification() -> Value {
        json!({
            "threadId": "t", "turnId": "u", "explanation": " Tune-up pass ",
            "plan": [
                { "step": "Prechecks", "status": "completed" },
                { "step": "Windows updates", "status": "inProgress" },
                { "step": "Scans", "status": "pending" },
                { "step": "  ", "status": "pending" },
            ]
        })
    }

    #[test]
    fn a_plan_round_trips_through_the_stored_item() {
        let plan = Plan::from_value(&notification()).expect("plan");
        assert_eq!(plan.steps.len(), 3, "blank steps are dropped");
        assert_eq!(plan.explanation.as_deref(), Some("Tune-up pass"));
        let item = plan.to_item();
        assert_eq!(item["type"], PLAN_ITEM_TYPE);
        assert_eq!(Plan::from_value(&item), Some(plan));
    }

    #[test]
    fn progress_and_the_current_step() {
        let plan = Plan::from_value(&notification()).expect("plan");
        assert_eq!((plan.done(), plan.complete()), (1, false));
        assert_eq!(plan.current().map(|s| s.step.as_str()), Some("Windows updates"));
        assert_eq!(
            plan.text(),
            "Plan (1 of 3 done)\nTune-up pass\n- [x] Prechecks\n- [ ] Windows updates (in progress)\n- [ ] Scans"
        );
    }

    #[test]
    fn carried_steps_lead_a_rebuilt_plan() {
        let before = Plan::from_value(&json!({ "plan": [
            { "step": "Open session + gather prior history", "status": "completed" },
            { "step": "Prechecks", "status": "completed" },
            { "step": "Windows updates", "status": "inProgress" },
        ] }))
        .expect("plan");
        let rebuilt = Plan::from_value(&json!({ "plan": [
            { "step": "Windows updates", "status": "completed" },
            { "step": "Scans", "status": "inProgress" },
            { "step": "Close session", "status": "pending" },
        ] }))
        .expect("plan");
        let shown = rebuilt.with_carried(&before.completed_steps());
        assert_eq!(
            shown.text(),
            "Plan (3 of 5 done)\n- [x] Open session + gather prior history\n- [x] Prechecks\n- [x] Windows updates\n- [ ] Scans (in progress)\n- [ ] Close session"
        );
    }

    #[test]
    fn a_carried_step_the_new_plan_names_keeps_the_new_status() {
        let carried = vec![PlanStep { step: "Run prechecks.".into(), status: PlanStatus::Completed }];
        let rebuilt = Plan::from_value(&json!({ "plan": [
            { "step": "run Prechecks", "status": "pending" },
            { "step": "Scans", "status": "pending" },
        ] }))
        .expect("plan");
        let shown = rebuilt.clone().with_carried(&carried);
        assert_eq!(shown, rebuilt);
        assert_eq!(shown.done(), 0);
    }

    #[test]
    fn no_steps_is_no_plan() {
        assert_eq!(Plan::from_value(&json!({ "plan": [] })), None);
        assert_eq!(Plan::from_value(&json!({ "type": "plan", "text": "- [x] a" })), None);
    }
}
