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
    out.push_str(
        "\nHOW THIS SESSION WORKS\n\
         - Every tool you have is a MasterTech tool; there is no shell, no file system and no web \
           here. Do not attempt to run commands on this host.\n\
         - Only this one machine is in scope. Pass its connection_string exactly as given; calls \
           for any other machine are refused.\n\
         - A technician may have to approve a tool call before it runs. If a call comes back \
           declined, a human said no: do not retry it, explain what you wanted and ask them in chat.\n\
         - When you need something only a human at the bench can tell you (what the customer \
           reported, what they see on screen, whether a part was swapped), ask it plainly in your \
           reply and end your turn; the technician answers in this chat.\n\
         - Keep replies short and concrete: symptom, evidence, verdict, next step. The technician \
           reads you between jobs.\n\n",
    );
}

/// The exact `create_diagnostic_session` arguments for this machine.
fn session_call(thread: &AgentThread, actor: &str) -> String {
    let mut args = vec![format!("connection_string `{}`", thread.connection_string)];
    if let Some(by) = &thread.requested_by {
        args.push(format!("requested_by `{by}`"));
    }
    if let Some(store) = &thread.store {
        args.push(format!("store `{store}`"));
    }
    args.push(format!("driven_by `{actor}`"));
    format!(
        "DIAGNOSTIC SESSION: call create_diagnostic_session with {} and nothing else identifying (no \
         customer_id, computer_id, customer_name, hostname or tech); it resolves the customer and the \
         computer from the connection_string itself.\n",
        args.join(", ")
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
