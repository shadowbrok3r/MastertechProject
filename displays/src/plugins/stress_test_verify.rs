//! Post-run verification that MCP-driven stress scripts landed rows in SurrealDB.

use database::schema::{
    entity_link::{parse_record_id, strip_surreal_key_quotes},
    RecordId, RecordIdExt, TestTool, COMPUTER_TABLE, HARDWARE_COMPONENT_TABLE,
    STRESS_TEST_RUN_TABLE,
};

/// Scripts that must persist via `stress-runner` (not plugin `burn_*` tools).
pub fn is_persisted_stress_script(script_name: &str) -> bool {
    stress_runner::is_stress_script(script_name)
}

/// Parse `stress_test_run id:` from client log lines.
pub fn extract_stress_run_id_from_logs(logs: &[String]) -> Option<String> {
    for line in logs {
        if let Some(idx) = line.find("stress_test_run id:") {
            let id = line[idx + "stress_test_run id:".len()..].trim();
            if !id.is_empty() {
                return Some(normalize_stress_run_id(id));
            }
        }
    }
    None
}

fn normalize_stress_run_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("{STRESS_TEST_RUN_TABLE}:{id}")
    }
}

fn normalize_session_key(session_id: &str) -> String {
    let s = session_id.trim();
    let s = s.strip_prefix("diagnostic_session:").unwrap_or(s);
    strip_surreal_key_quotes(s)
}

/// Clean `table:key` form with SurrealQL backtick quoting stripped.
fn canonical_id_string(raw: &str, table: &'static str) -> String {
    let rid = parse_record_id(raw, table);
    format!("{}:{}", rid.table, rid.key_string())
}

/// Resolve `computer:<HOST:hash9>` from a Web Console connection string.
pub async fn computer_id_for_connection(connection_string: &str) -> Option<String> {
    let mut response = database::DATABASE
        .query(
            "SELECT computer FROM connected_client \
             WHERE connection_string = $cs LIMIT 1",
        )
        .bind(("cs", connection_string.to_string()))
        .await
        .ok()?;
    let rows: Vec<serde_json::Value> = response.take(0).ok()?;
    rows.into_iter().next().and_then(|row| {
        row.get("computer")
            .and_then(|v| v.as_str().map(String::from))
    })
}

/// Verify `stress_test_run`, linked events, and `hardware_component` exist.
pub async fn verify_stress_test_persistence(
    computer_id: Option<&str>,
    run_id_hint: Option<&str>,
    expected_session_id: Option<&str>,
) -> serde_json::Value {
    let mut warnings: Vec<String> = Vec::new();

    let run_row = if let Some(hint) = run_id_hint {
        let rid = parse_record_id(hint, STRESS_TEST_RUN_TABLE);
        query_run_by_id(&rid).await
    } else if let Some(cid) = computer_id {
        query_latest_run_for_computer(cid).await
    } else {
        warnings.push("No computer id or run id hint — cannot verify persistence".into());
        None
    };

    let Some(run) = run_row else {
        return serde_json::json!({
            "verified": false,
            "warnings": warnings,
            "remediation": "No stress_test_run row found. If the client hung, call record_stress_test_run \
                to backfill. Do NOT use plugin burn_cpu/burn_memory/burn_disk — use \
                scripts_run_remote with category 'StressTests' (e.g. 'GPU Stress Test', 'QC Benchmark', \
                'Stress: CPU') on a client build with stress-runner persistence."
        });
    };

    let run_rid = run
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| parse_record_id(s, STRESS_TEST_RUN_TABLE));
    let run_id = run_rid
        .as_ref()
        .map(|r| format!("{}:{}", r.table, r.key_string()))
        .unwrap_or_default();
    let target = run
        .get("target_component")
        .and_then(|v| v.as_str())
        .map(|s| canonical_id_string(s, HARDWARE_COMPONENT_TABLE));
    let result = run
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let tool_label = run
        .get("tool_label")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // Scenario runs always write stage events; singles only write events on errors.
    let events_expected = TestTool::StressKitScenario { name: None }.label() == tool_label;

    let event_count = match run_rid.as_ref() {
        Some(rid) => count_events_for_run(rid).await.unwrap_or(0),
        None => 0,
    };
    if events_expected && event_count == 0 {
        warnings.push(
            "scenario stress_test_run has zero stress_test_event rows — stage transitions / \
             failures may not have been recorded"
                .into(),
        );
    }

    let hw_ok = if let Some(ref tc) = target {
        hardware_component_exists(tc).await
    } else {
        warnings.push("stress_test_run.target_component is NONE — hardware_component not linked".into());
        false
    };

    let session_key = run
        .get("session_ref")
        .and_then(|v| v.as_str())
        .map(normalize_session_key)
        .filter(|k| !k.is_empty());
    let session_linked = if let Some(expected) = expected_session_id.filter(|s| !s.is_empty()) {
        let expected_norm = normalize_session_key(expected);
        match session_key.as_deref() {
            None => {
                warnings.push(format!(
                    "stress_test_run.session_ref is unset — expected diagnostic_session:{expected_norm}"
                ));
                false
            }
            Some(run_norm) if run_norm != expected_norm => {
                warnings.push(format!(
                    "stress_test_run.session_ref ({run_norm}) does not match expected ({expected_norm})"
                ));
                false
            }
            Some(_) => true,
        }
    } else {
        true
    };

    let events_ok = !events_expected || event_count > 0;
    let verified = events_ok && target.is_some() && hw_ok && session_linked;
    if !verified && warnings.is_empty() {
        warnings.push("Persistence incomplete — see event_count and target_component".into());
    }

    serde_json::json!({
        "verified": verified,
        "run_id": run_id,
        "result": result,
        "failure_kind": run.get("failure_kind"),
        "tool_label": tool_label,
        "target_component": target,
        "session_ref": session_key.as_ref().map(|k| format!("diagnostic_session:{k}")),
        "session_linked": session_linked,
        "event_count": event_count,
        "events_expected": events_expected,
        "hardware_component_present": hw_ok,
        "warnings": warnings,
        "remediation": if verified { serde_json::Value::Null } else {
            serde_json::json!("Call record_stress_test_run to backfill missing rows, or re-run on a \
                client with current MasterTech (StressTests category uses stress-runner). Never substitute \
                plugin burn_* tools — they do not write stress_test_* tables.")
        }
    })
}

async fn query_run_by_id(id: &RecordId) -> Option<serde_json::Value> {
    let mut response = database::DATABASE
        .query("SELECT id, result, failure_kind, target_component, session_ref, tool_label, started_at FROM $id")
        .bind(("id", id.clone()))
        .await
        .ok()?;
    let rows: Vec<serde_json::Value> = response.take(0).ok()?;
    rows.into_iter().next()
}

async fn query_latest_run_for_computer(computer_key: &str) -> Option<serde_json::Value> {
    let cid = parse_record_id(computer_key, COMPUTER_TABLE);
    let mut response = database::DATABASE
        .query(
            "SELECT id, result, failure_kind, target_component, session_ref, tool_label, started_at FROM stress_test_run \
             WHERE computer = $c ORDER BY started_at DESC LIMIT 1",
        )
        .bind(("c", cid))
        .await
        .ok()?;
    let rows: Vec<serde_json::Value> = response.take(0).ok()?;
    rows.into_iter().next()
}

async fn count_events_for_run(run_ref: &RecordId) -> Option<u64> {
    let mut response = database::DATABASE
        .query("SELECT count() FROM stress_test_event WHERE run_ref = $r GROUP ALL")
        .bind(("r", run_ref.clone()))
        .await
        .ok()?;
    let rows: Vec<serde_json::Value> = response.take(0).ok()?;
    rows.into_iter()
        .next()
        .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
}

async fn hardware_component_exists(component_key: &str) -> bool {
    let cid = parse_record_id(component_key, HARDWARE_COMPONENT_TABLE);
    let response = database::DATABASE
        .query("SELECT id FROM $id")
        .bind(("id", cid))
        .await
        .ok();
    match response {
        Some(mut r) => r
            .take::<Vec<serde_json::Value>>(0)
            .map(|rows| !rows.is_empty())
            .unwrap_or(false),
        None => false,
    }
}
