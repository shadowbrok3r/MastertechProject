//! Scripts that are a sequence of other scripts.
//!
//! The children come from the catalog's `runs`, so the list a tech sees and the
//! list that executes are the same one. The two fan-outs this replaces were
//! hardcoded in their callers and had already drifted apart.
//!
//! Children run one at a time and the composite waits for each. The original
//! path started them all at once and returned immediately, so the queue moved on
//! as soon as any one of them logged.

use std::time::Instant;

use displays::scripts::catalog::{CATALOG, ScriptDef};
use displays::scripts::executor::{
    CancelToken, ScriptContext, ScriptExecutor, ScriptHandle, ScriptOutcome, ScriptResult,
};
use displays::scripts::id::ScriptId;

/// Composites this executor claims. `activate-cps` is deliberately absent: its
/// children have no executor yet, so it stays on the original path.
const HANDLED: &[&str] = &["run-prechecks", "run-junkware-category"];

pub struct CompositeExecutor;

impl ScriptExecutor for CompositeExecutor {
    fn handles(&self, id: &ScriptId) -> bool {
        HANDLED.contains(&id.as_str())
    }

    fn spawn(
        &self,
        def: &ScriptDef,
        ctx: &ScriptContext,
        run_token: u64,
        cancel: CancelToken,
    ) -> ScriptHandle {
        let (tx, done) = crossbeam::channel::bounded(1);
        let def = def.clone();
        let ctx = ctx.clone();
        let started = Instant::now();
        let child_cancel = cancel.clone();

        std::thread::spawn(move || {
            let result = run(&def, &ctx, &child_cancel);
            let _ = tx.send(ScriptOutcome::plain(
                def.id.clone(),
                run_token,
                result,
                started.elapsed(),
            ));
        });

        ScriptHandle {
            run_token,
            done,
            cancel,
        }
    }
}

fn run(def: &ScriptDef, ctx: &ScriptContext, cancel: &CancelToken) -> ScriptResult {
    let (category, name) = (def.category(), def.name.as_str());
    let registry = super::registry();

    let mut succeeded = 0;
    let mut warned = 0;
    let mut failed = 0;

    for (index, child_id) in def.runs.iter().enumerate() {
        if cancel.is_cancelled() {
            let msg = format!("Stopped after {index} of {} steps", def.runs.len());
            ctx.log_warning(category, name, msg.clone());
            return ScriptResult::Warning(msg);
        }

        let Some(child) = CATALOG.get(child_id) else {
            failed += 1;
            ctx.log_error(
                category.clone(),
                name,
                format!("'{child_id}' is not in the catalog"),
            );
            continue;
        };

        if registry.find(child_id).is_none() {
            failed += 1;
            ctx.log_error(
                category.clone(),
                name,
                format!("no executor claims '{child_id}'"),
            );
            continue;
        }

        // Children share the parent's token, so Stop reaches a running one.
        // Each child logs under its own name, so the composite only counts.
        let handle = registry.spawn(child, ctx, 0, cancel.clone());
        match handle.done.recv() {
            Ok(outcome) => match outcome.result {
                ScriptResult::Success(_) => succeeded += 1,
                ScriptResult::Warning(_) | ScriptResult::Skipped(_) => warned += 1,
                ScriptResult::Error(_) => failed += 1,
            },
            Err(_) => {
                failed += 1;
                ctx.log_error(
                    category.clone(),
                    name,
                    format!("'{}' stopped without reporting", child.name),
                );
            }
        }
    }

    let msg = format!("{succeeded} succeeded, {warned} inconclusive, {failed} failed");
    if failed > 0 {
        ctx.log_error(category, name, msg.clone());
        ScriptResult::Error(msg)
    } else if warned > 0 {
        ctx.log_warning(category, name, msg.clone());
        ScriptResult::Warning(msg)
    } else {
        ctx.log_success(category, name, msg.clone());
        ScriptResult::Success(msg)
    }
}

#[cfg(test)]
mod composite_executor_tests {
    use super::*;

    #[test]
    fn every_handled_composite_declares_children() {
        for id in HANDLED {
            let def = CATALOG
                .get(&ScriptId::new(*id))
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert!(!def.runs.is_empty(), "{id} declares no children");
        }
    }

    /// A composite whose child has no executor would report every step failing.
    #[test]
    fn every_child_of_a_handled_composite_has_an_executor() {
        let registry = super::super::registry();
        for id in HANDLED {
            let def = CATALOG.get(&ScriptId::new(*id)).expect("catalog entry");
            for child in &def.runs {
                assert!(
                    registry.find(child).is_some(),
                    "{id} runs {child}, which has no executor"
                );
            }
        }
    }

    /// Children are run inline on the composite's thread, so a child that is
    /// itself a composite would nest without bound.
    #[test]
    fn no_handled_composite_runs_another_composite() {
        for id in HANDLED {
            let def = CATALOG.get(&ScriptId::new(*id)).expect("catalog entry");
            for child in &def.runs {
                let child_def = CATALOG.get(child).expect("child in the catalog");
                assert!(
                    child_def.runs.is_empty(),
                    "{id} runs {child}, which is itself a composite"
                );
            }
        }
    }

    /// Children run one at a time, so a composite whose budget is under a
    /// child's is cancelled before that child can finish.
    #[test]
    fn a_composite_outlives_its_slowest_child() {
        for id in HANDLED {
            let def = CATALOG.get(&ScriptId::new(*id)).expect("catalog entry");
            let budget = CATALOG.timeout_secs(&def.name).unwrap_or_default();
            for child in &def.runs {
                let child_def = CATALOG.get(child).expect("child in the catalog");
                let child_budget = CATALOG.timeout_secs(&child_def.name).unwrap_or_default();
                assert!(
                    budget >= child_budget,
                    "{id} allows {budget}s but {child} needs up to {child_budget}s"
                );
            }
        }
    }

    /// The junkware sweep must stay the nine the tab ran, not the terminal's
    /// ten - the two lists disagreed over ESET.
    #[test]
    fn the_junkware_sweep_is_unchanged() {
        let def = CATALOG
            .get(&ScriptId::new("run-junkware-category"))
            .expect("catalog entry");
        let names: Vec<&str> = def
            .runs
            .iter()
            .map(|id| CATALOG.get(id).expect("child").name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "OneLaunch",
                "WebNavigator Browser",
                "Wave Browser",
                "Clear Browser",
                "Shift Browser",
                "Avast Browser",
                "Mcaffee Safe",
                "Driver Support",
                "Winzip",
            ]
        );
    }

    #[test]
    fn the_executor_claims_only_what_it_runs() {
        let executor = CompositeExecutor;
        for id in HANDLED {
            assert!(executor.handles(&ScriptId::new(*id)));
        }
        assert!(!executor.handles(&ScriptId::new("activate-cps")));
        assert!(!executor.handles(&ScriptId::new("onelaunch")));
    }
}
