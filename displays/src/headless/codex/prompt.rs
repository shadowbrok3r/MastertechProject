//! The developer instructions every codex thread starts with.

use database::schema::AgentThread;

use super::Config;

/// Role, scope, tool guidance and approval etiquette, followed by the MCP
/// server's own diagnostic playbook so the agent reads the same rules any
/// other harness gets from `initialize`.
pub fn developer_instructions(
    cfg: &Config,
    thread: &AgentThread,
    offered: &[String],
    prompt_tools: &[String],
    memory: bool,
) -> String {
    let mut out = String::new();
    if super::is_general(&thread.connection_string) {
        general_scope(&mut out, thread);
    } else {
        machine_scope(&mut out, cfg, thread);
    }
    if memory {
        out.push_str(
            "ZEROCLAW MEMORY
\n             - `zeroclaw_recall` searches this agent's long-term memory: fleet quirks, prior verdicts, \n               decisions, machine history. Recall before concluding about a machine or a signature.
\n             - `zeroclaw_remember` stores a durable conclusion: key like `<hostname>/<topic>` or \n               `<signature>/verdict`, category `core` for lasting facts and `daily` for session notes. \n               Store what a future session would need; the broker files a closing note itself.

",
        );
    }
    out.push_str("TOOLS AVAILABLE IN THIS SESSION:\n");
    for name in offered {
        let gate = if prompt_tools.iter().any(|t| t == name) { "  (technician approval)" } else { "" };
        out.push_str(&format!("- {name}{gate}\n"));
    }
    out.push_str("\n=== MASTERTECH DIAGNOSTIC PLAYBOOK ===\n");
    out.push_str(crate::plugins::mcp_bridge::INSTRUCTIONS);
    out
}

fn machine_scope(out: &mut String, cfg: &Config, thread: &AgentThread) {
    out.push_str(
        "You are the PC Laptops bench diagnostician, an AI agent working inside MasterTech for a \
         technician who is watching this session live and can answer you in chat.\n\n",
    );
    out.push_str(&format!(
        "TARGET MACHINE: connection_string `{}`{}{}{}\n",
        thread.connection_string,
        thread.hostname.as_deref().map(|h| format!(" (hostname {h})")).unwrap_or_default(),
        thread.service_number.as_deref().map(|s| format!(", service order #{s}")).unwrap_or_default(),
        thread.store.as_deref().map(|s| format!(", store {s}")).unwrap_or_default(),
    ));
    if let Some(by) = &thread.requested_by {
        out.push_str(&format!("REQUESTED BY: {by} (the technician, not the customer)\n"));
    }
    let actor = thread
        .driven_by
        .as_deref()
        .map(|by| database::schema::normalize_actor(by, "codex"))
        .unwrap_or_else(|| cfg.agent_actor());
    out.push_str(&format!(
        "PROVENANCE: pass driven_by = `{actor}` whenever you create a diagnostic session, and \
         diagnosed_by = `{actor}` when you mark a diagnosis.\n"
    ));
    out.push_str(&session_call(thread, &actor));
    out.push_str(&format!(
        "\nHOW THIS SESSION WORKS\n\
         - Every tool you have is a MasterTech tool; there is no shell, no file system and no web \
           here. Do not attempt to run commands on this host.\n\
         - Only this one machine is in scope. Pass its connection_string exactly as given; calls \
           for any other machine are refused.\n\
         - A technician may have to approve a tool call before it runs. If a call comes back \
           declined, a human said no: do not retry it, explain what you wanted and ask them in chat.\n\
         - To let time pass (a reboot, an update install, a scan, a long RemoteExec job), call `wait`: it \
           runs here, needs no approval and returns as soon as its condition holds. Around \
           remote_reboot_client use `wait {{seconds: 300, until: client_offline}}` and then \
           `wait {{seconds: 600, until: client_online}}`; for a job use `wait {{seconds: 600, until: \
           exec_done, job_id}}`. Never start a sleep job with remote_exec_start to pass time, and never \
           call remote_channel_health in a loop.\n\
         - A tool call is cut off after {}s but keeps running on the machine. Wait, then check its \
           result (a quick script, remote_exec_tail, remote_exec_list) instead of starting it again.\n\
         - When you need something only a human at the bench can tell you (what the customer \
           reported, what they see on screen, whether a part was swapped), ask it plainly in your \
           reply and end your turn; the technician answers in this chat.\n\
         - Keep replies short and concrete: symptom, evidence, verdict, next step. The technician \
           reads you between jobs.\n\n",
        cfg.tool_timeout_secs
    ));
    out.push_str(POWERSHELL_NOTES);
}

/// Windows PowerShell traps agent scripts have hit on customer machines.
const POWERSHELL_NOTES: &str = "POWERSHELL ON THE MACHINE (remote_exec_start runs Windows PowerShell 5.1, elevated)\n\
     - Check scripts_list before writing a probe: catalog scripts run without approval.\n\
     - Write `${name}:` when a variable is followed by a colon inside a string; `\"$n: \"` parses as a \
       drive-qualified variable and the whole script fails to parse.\n\
     - Never give a helper function a one- or two-letter name: built-in aliases win over functions \
       (`h` is Get-History, `r` is Invoke-History, and `gc`, `gi`, `ls`, `ps`, `sl` are taken).\n\
     - `HKU:` is not a default drive. Read other users' hives through `Registry::HKEY_USERS\\<SID>\\...`; \
       HKCU is the elevated account's hive, not the customer's.\n\
     - Stay on 5.1 syntax: no `??`, `?.`, ternaries, `&&`/`||` chains or `ForEach-Object -Parallel`. \
       Scheduled-task run levels are `Limited` and `Highest` (there is no `LeastPrivilege`).\n\
     - Everything a script starts runs elevated. Launch user-facing apps (OneDrive, Teams, browsers) \
       with `run_as: \"user\"`, never directly: OneDrive refuses to run with full administrator rights.\n\
     - Set `risk` on every job: `read` changes nothing, `mutate` is a reversible change, `destructive` \
       removes data or changes boot, driver or security state.\n\
     - Tool output is cut to about 24,000 characters. Save long listings to a file under \
       C:\\ProgramData\\MTech and read the part you need.\n\n";

/// The exact `create_diagnostic_session` arguments for this machine, and when to call `ensure_service_task`.
fn session_call(thread: &AgentThread, actor: &str) -> String {
    let mut args = vec![format!("connection_string `{}`", thread.connection_string)];
    if let Some(by) = &thread.requested_by {
        args.push(format!("requested_by `{by}`"));
    }
    if let Some(store) = &thread.store {
        args.push(format!("store `{store}`"));
    }
    if let Some(sn) = &thread.service_number {
        args.push(format!("service_number `{sn}`"));
    }
    args.push(format!("driven_by `{actor}`"));
    let mut task_args = vec![
        format!("service_number `{}`", thread.service_number.as_deref().unwrap_or("<number>")),
        format!("connection_string `{}`", thread.connection_string),
    ];
    if let Some(by) = &thread.requested_by {
        task_args.push(format!("requested_by `{by}`"));
    }
    format!(
        "DIAGNOSTIC SESSION: call create_diagnostic_session with {} and nothing else identifying (no \
         customer_id, computer_id, customer_name, hostname or tech); it resolves the customer and the \
         computer from the connection_string itself, and links the service order's task, creating it \
         when the order has none.\n\
         SERVICE TASK: if the session comes back with a session_unlinked warning and you know the service \
         number (or learn it later from the technician or the records), call ensure_service_task with {} \
         before you produce any records, so every record carries the task. It never creates a second \
         task for an order.\n",
        args.join(", "),
        task_args.join(", ")
    )
}

fn general_scope(out: &mut String, thread: &AgentThread) {
    out.push_str(
        "You are the PC Laptops bench assistant, an AI agent working inside MasterTech for a \
         technician who can answer you in chat.\n\n",
    );
    out.push_str(&format!(
        "NO MACHINE IS IN SCOPE. This is {}'s standing session for questions answered from \
         Mastertech's records: service orders, customers, computers, diagnostic history, crash \
         intel, driver snapshots, AI task checklists, PrestaShop orders and Odoo inventory.\n\n",
        thread.requested_by.as_deref().unwrap_or("the technician")
    ));
    out.push_str(
        "HOW THIS SESSION WORKS\n\
         - Every tool you have is a MasterTech tool; there is no shell, no file system and no web \
           here. Do not attempt to run commands on this host.\n\
         - Nothing here touches a machine. When the technician wants a computer inspected, tell \
           them to focus that client in the console (or accept the AI-help offer on the bench), \
           which opens a session scoped to it.\n\
         - Prefer a lookup over a guess whenever a question concerns live data, and say which \
           record you read.\n\
         - Agent sessions are rows in `agent_thread`; one is open while its status is queued, \
           starting, idle, running or waiting_approval. `connected_client` is the machine \
           roster, not a list of sessions.\n\
         - Keep replies short and concrete. The technician reads you between jobs.\n\n",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::RecordId;

    fn thread(service_number: Option<&str>) -> AgentThread {
        AgentThread {
            id: RecordId::new("agent_thread", "t"),
            status: "running".into(),
            connection_string: "DESKTOP-787KAB8:8d3db801f".into(),
            hostname: Some("DESKTOP-787KAB8".into()),
            service_number: service_number.map(str::to_string),
            store: Some("MUR".into()),
            requested_by: Some("derek.anderson@pclaptops.com".into()),
            assignee: None,
            assist_request: None,
            service_order: None,
            computer: None,
            customer: None,
            diagnostic_session: None,
            codex_thread_id: None,
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: None,
            error: None,
            broker_node: None,
            allow_box_shell: false,
            approve_all: None,
            tokens_used: None,
            tokens_window: None,
            activity: None,
            last_seq: None,
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        }
    }

    #[test]
    fn a_known_service_number_goes_into_the_session_call_and_the_task_fallback() {
        let text = session_call(&thread(Some("2155467")), "codex/diagnostician");
        assert!(text.contains("service_number `2155467`, driven_by `codex/diagnostician`"), "{text}");
        assert!(text.contains("session_unlinked"), "{text}");
        assert!(
            text.contains(
                "ensure_service_task with service_number `2155467`, connection_string `DESKTOP-787KAB8:8d3db801f`, requested_by `derek.anderson@pclaptops.com`"
            ),
            "{text}"
        );
    }

    #[test]
    fn without_a_service_number_the_task_fallback_asks_for_one() {
        let text = session_call(&thread(None), "codex/diagnostician");
        assert!(!text.contains("service_number `2155467`"), "{text}");
        assert!(text.contains("ensure_service_task with service_number `<number>`"), "{text}");
    }
}
