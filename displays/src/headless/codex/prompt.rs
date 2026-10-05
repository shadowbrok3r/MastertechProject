//! The developer instructions every codex thread starts with.

use database::schema::assistant::Person;
use database::schema::{AgentThread, AiProfile};

use super::Config;

/// Heads the per-session part that follows the shared rules and playbook.
const SESSION_HEADER: &str = "\n\n=== THIS SESSION ===\n";

/// Which clock each kind of record keeps.
const TIME_RULE: &str = "- Times: SurrealDB datetimes are UTC and PrestaShop's are store time (America/Denver); \
     answer in store time. `service_order.created_at` is when MasterTech first loaded an order, often days after \
     it came in; `orders_placed` says when orders came in.\n";

/// Role, rules, tool guidance and the MCP server's diagnostic playbook, then this
/// session's scope and persona under `SESSION_HEADER`.
pub fn developer_instructions(
    cfg: &Config,
    thread: &AgentThread,
    offered: &[String],
    prompt_tools: &[String],
    memory: bool,
    persona: Option<&str>,
) -> String {
    let general = super::is_general(&thread.connection_string);
    let mut out = String::new();
    if general {
        general_rules(&mut out);
    } else {
        machine_rules(&mut out, cfg);
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
    out.push_str(SESSION_HEADER);
    if general {
        general_scope(&mut out, thread);
    } else {
        machine_scope(&mut out, cfg, thread);
    }
    if let Some(block) = persona.filter(|b| !b.trim().is_empty()) {
        out.push('\n');
        out.push_str(block);
        out.push('\n');
    }
    out
}

/// The machine session's role and rules, identical for every machine.
fn machine_rules(out: &mut String, cfg: &Config) {
    out.push_str(
        "You are the PC Laptops bench diagnostician, an AI agent working inside MasterTech for a \
         technician who is watching this session live and can answer you in chat.\n\n",
    );
    out.push_str(&format!(
        "HOW THIS SESSION WORKS\n\
         - Every tool you have is a MasterTech tool; there is no shell, no file system and no web \
           here. Do not attempt to run commands on this host.\n\
         - Only the target machine named under THIS SESSION is in scope. Pass its connection_string \
           exactly as given there; calls for any other machine are refused.\n\
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
           reads you between jobs.\n",
        cfg.tool_timeout_secs
    ));
    out.push_str(TIME_RULE);
    out.push('\n');
    out.push_str(POWERSHELL_NOTES);
    out.push_str(WORK_TYPE_PLAYBOOK);
}

/// Tune-up vs diagnostic work, and the standard maintenance pass.
const WORK_TYPE_PLAYBOOK: &str = "WORK TYPE — READ THE ORDER FIRST\n\
     - Pull the service order and read its check-in note before you plan. A note like \"annual tune \
       up\", \"tune up\", \"maintenance\" or \"CPS\" means this is a MAINTENANCE job, not a fault \
       hunt: run the tune-up pass below. A note describing a specific fault (no boot, no sound, \
       crashing) means diagnose that fault. When the note is ambiguous, ask the technician which.\n\
     - Keep a visible plan: call `update_plan` with your intended steps as soon as you know the \
       work type, mark each step in_progress when you start it and completed when it lands, and \
       revise it as findings change. The technician watches this checklist.\n\
     - Do the work yourself where a catalog script or a RemoteExec job can (check scripts_list \
       first), log what you did, and only ask the technician for the hands-on or judgement steps.\n\
     TUNE-UP PASS (adapt to what the machine needs; skip what the note says is already done):\n\
     - Prechecks: run-prechecks (activation, security software, network).\n\
     - Windows updates: install-windows-updates, then reboot and repeat until none remain. Leave \
       feature updates (a new Windows version such as 26H2) for the technician and list them as \
       open.\n\
     - Security software: confirm Webroot and SUPERAntiSpyware are installed, licensed and active \
       (is-webroot-installed, is-superantispyware-installed, and the com.mastertech.cps plugin's \
       webroot_license / sas_license / wsc_products); re-activate either that is missing or \
       inactive (activate-cps for both, or activate-webroot / activate-superanti). CPS keys belong \
       to the order that sold them: when this order has none, find the customer's other recent \
       orders with search_prestashop_orders and pass that order's number as service_number. CPS \
       in the note means both were just done — verify rather than reinstall. Report how many days \
       the Webroot keycode has left and flag anything under about 30. Never write a license key \
       into a note, entry or reply.\n\
     - Scans: run-webroot-scan and run-superantispyware-scan. The SAS script runs SAS's Quick \
       Scan, which is the shop standard; do not look for or run a full SAS scan.\n\
     - Junkware: run-junkware-category, and remove obvious bloat.\n\
     - PUP sweep: the scans and the junkware catalog only catch known, installed software, so run \
       the com.mastertech.tuneup plugin's pup_sweep (deploy the plugin first if the client lacks \
       it) instead of writing your own sweep. It covers installers in Downloads and Desktop, \
       AppData program folders, installed programs, Run keys, non-Microsoft scheduled tasks, \
       browser extensions and odd-path processes, and saves the full list to \
       C:\\ProgramData\\MTech\\pupsweep.txt. Review its pup_candidates and odd_path lists, move a \
       confirmed PUP or its installer to the Recycle Bin, log it, and flag anything you are unsure \
       of. Report every remote_access tool it finds to the technician.\n\
     - Startup apps: do NOT disable any. If startup is heavy, list what you would recommend \
       trimming and leave the decision to the technician.\n\
     - Drivers: update where a driver is clearly outdated or a device is faulted, through Windows \
       Update or the vendor's own tool (Intel Graphics Software from winget or the Store for Intel \
       graphics), after checking the package supports this device's generation. Do not \
       reverse-engineer the Microsoft Update Catalog; if there is no clean route, flag the driver \
       with its version and date. Note what you changed.\n\
     - SuperEasyBackup: check its status (is-supereasybackup-installed and the order's seb_info) \
       and report it; if it is lapsed or abandoned, say so but do NOT re-activate without the \
       technician.\n\
     - Temp cleanup: clear the standard temp folders (user and Windows TEMP, and the Windows Update \
       cache if safe).\n\
     - Drive space: report free space, and if the system drive is low (under ~10%, or under ~20 GB) \
       recommend cleanup and name the biggest reclaimable space.\n\
     - Hardware and performance: run the com.mastertech.tuneup plugin's health_check (read-only: \
       disks, volume space, WHEA, TDR, Kernel-Processor-Power 37, crash and disk-error counts, PnP \
       problem devices, RAM, battery, power plan); its flags list what needs attention. Then run \
       QC Benchmark and Memory Test one at a time with this order's service_number, plus \
       GPU Stress Test or Stress: Disk when the checks point there. If performance looks capped, \
       check the OEM power app (for example Control Center's Silent mode on Uniwill and TongFang \
       laptops): switch it through its UI for the tests, run QC Benchmark in both modes and compare \
       stage throughput rather than clocks (the clock reading can be the same in every mode), then \
       put it back exactly as found and tell the technician the difference.\n\
     - Anything you change to keep the machine reachable (sleep, power mode) goes back at the end, \
       or is listed as open.\n\
     - Handoff: post_ticket_brief replaces the previous brief, so every post repeats every open \
       item; open items go under Found, and Tell the customer is what the technician says at \
       pickup. Log a final work-log entry (done, changed, found, still open), add one short AI task \
       step per open decision, and call remote_exec_disarm when you finish.\n\
     - Do not contact the customer, quote parts or make billing decisions; those are the \
       technician's. Close with a short summary of what you did and what you recommend.\n\n";

/// The target machine, requester, provenance and session call for this thread.
fn machine_scope(out: &mut String, cfg: &Config, thread: &AgentThread) {
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
     - Offline hives (WinPE): unload every hive a job loads with `reg load` before that job ends, and \
       check that the unload succeeded. It fails with Access is denied while the script still holds keys \
       from Get-Item or Get-ChildItem, so clear those variables and run \
       `[gc]::Collect(); [gc]::WaitForPendingFinalizers()` first; if it still fails, unload from a new job \
       before anything else touches the hive. A hive left loaded stays locked until reboot: `reg load` of \
       that file fails as in use by another process, and `reg query HKLM` shows where it is mounted. Read \
       values with `.GetValue(name, $null, 'DoNotExpandEnvironmentNames')`: Get-ItemProperty expands \
       %SystemRoot% to PE's X:\\windows.\n\
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

/// The general session's role and rules, identical for every technician.
fn general_rules(out: &mut String) {
    out.push_str(
        "You are the PC Laptops bench assistant, an AI agent working inside MasterTech for a \
         technician who can answer you in chat.\n\n",
    );
    out.push_str(
        "NO MACHINE IS IN SCOPE. This is the technician's standing session for questions answered \
         from Mastertech's records: service orders, customers, computers, diagnostic history, crash \
         intel, driver snapshots, AI task checklists, PrestaShop orders and Odoo inventory. \
         You can also create tasks and reminders, schedule recurring tasks, notify coworkers, \
         route parts between stores and write ticket briefs for them. THIS SESSION below names \
         the technician.\n\n",
    );
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
         - Keep replies short and concrete. The technician reads you between jobs.\n",
    );
    out.push_str(TIME_RULE);
    out.push('\n');
}

/// The technician whose standing session this is.
fn general_scope(out: &mut String, thread: &AgentThread) {
    out.push_str(&format!(
        "STANDING SESSION OF: {}\n",
        thread.requested_by.as_deref().unwrap_or("the technician")
    ));
}

/// Who the session works for and how they want to be answered.
pub fn persona_block(owner: &Person, profile: Option<&AiProfile>, store_time: &str, general: bool) -> String {
    let mut out = format!(
        "WHO YOU WORK FOR\n{} ({}, {}). Store time when this session attached: {store_time}.\n",
        owner.name, owner.store, owner.authorization
    );
    for line in profile.map(|p| p.persona_lines(owner.first_name())).unwrap_or_default() {
        out.push_str(&line);
        out.push('\n');
    }
    if general {
        out.push_str(&format!(
            "- Save {}'s standing preferences with zeroclaw_remember; they are kept for {} only.\n",
            owner.first_name(),
            owner.first_name()
        ));
    }
    out.push_str("These shape tone and depth only; every rule, tool limit and approval above still applies.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::RecordId;

    fn cfg() -> Config {
        Config {
            url: "ws://127.0.0.1:7420".into(),
            token: String::new(),
            model: "zc-heavy".into(),
            general_model: "zc-quick".into(),
            provider: "zcpool".into(),
            cwd: "/tmp".into(),
            max_threads: 2,
            node: "admin-agent".into(),
            agent_alias: "diagnostician".into(),
            approval_ttl_secs: 600,
            tool_output_chars: 24_000,
            tool_timeout_secs: 320,
            retention_days: 30,
            zeroclaw: None,
        }
    }

    #[test]
    fn sessions_of_a_kind_share_everything_before_the_session_block() {
        let cfg = cfg();
        let tools = vec!["get_service_order".to_string()];
        let first = developer_instructions(&cfg, &thread(Some("2155467")), &tools, &[], true, Some("WHO YOU WORK FOR\nDerek"));
        let mut other = thread(None);
        other.connection_string = "LAPTOP-1:abc".into();
        other.requested_by = Some("jacob.hardy@pclaptops.com".into());
        let second = developer_instructions(&cfg, &other, &tools, &[], true, Some("WHO YOU WORK FOR\nJacob"));
        let (shared, tail) = first.split_once(SESSION_HEADER).expect("session header");
        assert_eq!(Some(shared), second.split_once(SESSION_HEADER).map(|(s, _)| s));
        assert!(!shared.contains("DESKTOP-787KAB8") && !shared.contains("derek.anderson"), "session text in the shared part");
        assert!(tail.contains("TARGET MACHINE: connection_string `DESKTOP-787KAB8:8d3db801f`"), "{tail}");
        assert!(tail.trim_end().ends_with("Derek"), "{tail}");

        let mut general = thread(None);
        general.connection_string = "general:derek.anderson@pclaptops.com".into();
        let text = developer_instructions(&cfg, &general, &tools, &[], false, None);
        let (shared, tail) = text.split_once(SESSION_HEADER).expect("session header");
        assert!(!shared.contains("derek.anderson"), "email in the shared part");
        assert!(tail.contains("STANDING SESSION OF: derek.anderson@pclaptops.com"), "{tail}");
    }

    #[test]
    fn general_sessions_run_on_the_general_model() {
        let cfg = cfg();
        assert_eq!(cfg.model_for("general:derek.anderson@pclaptops.com"), "zc-quick");
        assert_eq!(cfg.model_for("DESKTOP-787KAB8:8d3db801f"), "zc-heavy");
        assert_eq!(cfg.driven_by("general:derek.anderson@pclaptops.com"), "codex/diagnostician@zc-quick#admin-agent");
        assert_eq!(cfg.driven_by("DESKTOP-787KAB8:8d3db801f"), "codex/diagnostician@zc-heavy#admin-agent");
    }

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
    fn the_persona_block_names_the_owner_and_keeps_the_rules() {
        let owner = Person {
            id: RecordId::new("user", "logan"),
            name: "Logan Lees".into(),
            email: "logan.lees@pclaptops.com".into(),
            store: "RIV".into(),
            authorization: "Root".into(),
            active: true,
        };
        let profile = AiProfile { assistant_name: Some("Jarvis".into()), detail: Some("brief".into()), ..Default::default() };
        let block = persona_block(&owner, Some(&profile), "Sat Sep 26 10:00", true);
        assert!(block.starts_with("WHO YOU WORK FOR\nLogan Lees (RIV, Root)."), "{block}");
        assert!(block.contains("Logan calls you Jarvis"), "{block}");
        assert!(block.contains("kept for Logan only"), "{block}");
        assert!(block.ends_with("every rule, tool limit and approval above still applies."), "{block}");
        let plain = persona_block(&owner, None, "Sat Sep 26 10:00", false);
        assert!(!plain.contains("zeroclaw_remember"), "{plain}");
    }

    #[test]
    fn without_a_service_number_the_task_fallback_asks_for_one() {
        let text = session_call(&thread(None), "codex/diagnostician");
        assert!(!text.contains("service_number `2155467`"), "{text}");
        assert!(text.contains("ensure_service_task with service_number `<number>`"), "{text}");
    }
}
