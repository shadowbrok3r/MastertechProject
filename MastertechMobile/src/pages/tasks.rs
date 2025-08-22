use dioxus::prelude::*;
use chrono::Utc;
use crossbeam_channel::unbounded;
use database::live_data::{listen_data, handle_live_data};
use database::schema::{LiveTaskPayload, User, Priority, Status, TicketData, ComputerData, CustomerData};
use database::schema::utilities::get_prestashop_payload;
use database::schema::prestashop_schema::PrestashopPayload;
use database::schema::task::filter::FilterLiveTasks; // fuzzy helper trait
use crate::services::tasks::{
    fetch_incomplete_tasks, fetch_completed_tasks,
    fetch_task_notes, fetch_store_users, toggle_completed, update_status, update_assignee, add_note,
    NewTaskInput, create_task_simple,
};

// =========================
// Task Board Props
// =========================
#[derive(Props, PartialEq, Clone)]
pub struct TaskBoardProps {
    pub page: String,
    #[props(default)]
    pub on_navigate: Option<Callback<String>>,
    #[props(default)]
    pub refresh_token: u64,
    #[props(default)]
    pub create_task_trigger: u64,
}

// =========================
// Task Board Root
// =========================
#[component]
pub fn TaskBoard(props: TaskBoardProps) -> Element {
    // Core signals
    let all_tasks = use_signal(|| Vec::<LiveTaskPayload>::new());
    let last_err = use_signal(|| Option::<String>::None);
    let current_user_res = use_resource(|| async move { current_user().await });
    let store_users = use_resource(|| async move { Ok::<_, anyhow::Error>(fetch_store_users().await) });

    // Refresh / page change fetch
    {
            let page = props.page.clone();
            let refresh = props.refresh_token; // capture to re-run when changed
            let mut all_sig = all_tasks.to_owned();
            let mut err_sig = last_err.to_owned();
            use_future(move || {
                let page_clone = page.clone();
                async move {
                    let res = if page_clone == "Completed Tasks" { fetch_completed_tasks().await } else { fetch_incomplete_tasks().await };
                    match res { Ok(list) => { err_sig.set(None); all_sig.set(list); } Err(e) => err_sig.set(Some(e.to_string())) }
                }
            });
            let _ = refresh; // silence unused
    }

    // Live updates
    {
        let mut all_sig = all_tasks.to_owned();
        use_effect(move || {
            let (tx, rx) = unbounded::<(surrealdb::Action, LiveTaskPayload)>();
            // Spawn the DB listener (async stream)
            spawn(async move { let _ = listen_data::<LiveTaskPayload>(tx, "task").await; });
            // Spawn a lightweight polling task instead of blocking recv() which can deadlock in WASM/mobile envs
            // Unified async polling loop (single-threaded) – avoids blocking recv()
            spawn(async move {
                loop {
                    while let Ok(msg) = rx.try_recv() {
                        let mut v = all_sig();
                        let _ = handle_live_data(msg, &mut v);
                        all_sig.set(v);
                    }
                    // Delay ~120ms
                    #[cfg(target_arch = "wasm32")]
                    { gloo_timers::future::TimeoutFuture::new(120).await; }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        use std::time::Duration;
                        futures_timer::Delay::new(Duration::from_millis(120)).await;
                    }
                }
            });
        });
    }

    // Modals & selection
    let show_task_modal = use_signal(|| false);
    let selected_task = use_signal(|| Option::<LiveTaskPayload>::None);
    // Mutable because we set it directly in handlers
    let mut show_create_modal = use_signal(|| false);
    {
        let trigger = props.create_task_trigger;
        let mut show = show_create_modal.to_owned();
    let mut last_seen = use_signal(|| 0u64);
    use_effect(move || { if trigger > 0 && trigger != last_seen() { last_seen.set(trigger); show.set(true); } });
    }
    let on_open_task = { let mut sel = selected_task.to_owned(); let mut show = show_task_modal.to_owned(); Callback::new(move |t: LiveTaskPayload| { sel.set(Some(t)); show.set(true); }) };
    let on_change_cb = { let mut all = all_tasks.to_owned(); Callback::new(move |updated: LiveTaskPayload| { let mut v = all(); if let Some(i)=v.iter().position(|x| x.id==updated.id){ v[i]=updated; } else { v.push(updated); } all.set(v); }) };

    // Derive columns
    let list_vec = all_tasks();
    let user_opt = current_user_res.read().as_ref().and_then(|r| r.as_ref().ok().cloned());
    let (users_ok, users_err) = match store_users.read().as_ref() { Some(Ok(u)) => (Some(u.clone()), None), Some(Err(e)) => (None, Some(e.to_string())), None => (None, None) };
    let status_columns: [&str;5] = ["Todo","In Repair","QC","Sales","Complete"]; // order
    let mut board_cols: Vec<(String, Vec<LiveTaskPayload>)> = if let Some(users) = users_ok.clone() {
        match props.page.as_str() {
            "My Tasks" => {
                let mut v = Vec::new();
                for col in status_columns { let tasks: Vec<LiveTaskPayload> = list_vec.iter().cloned().filter(|t| { let owner_ok = if let Some(u)=&user_opt { t.assignee==u.get_id() } else { true }; owner_ok && !t.completed && t.status.as_str()==col }).collect(); v.push((col.to_string(), tasks)); }
                v
            }
            "Store Tasks" | "Completed Tasks" => {
                use std::collections::HashMap; let my_store = user_opt.as_ref().map(|u| u.get_store()); let mut groups: HashMap<String, Vec<LiveTaskPayload>> = HashMap::new();
                for t in list_vec.iter().cloned() { if props.page=="Completed Tasks" && !t.completed { continue; } if props.page=="Store Tasks" && t.completed { continue; }
                    let uname = users.iter().find(|u| u.get_id()==t.assignee).and_then(|u| { if let Some(s)=my_store { if u.get_store()!=s { return None; } } Some(u.get_username().to_string()) }).unwrap_or_else(|| "Unassigned".into()); groups.entry(uname).or_default().push(t); }
                let mut keys: Vec<String> = groups.keys().cloned().collect(); keys.sort(); let mut v=Vec::new(); for k in keys { v.push((k.clone(), groups.remove(&k).unwrap_or_default())); } v }
            _ => Vec::new(),
        }
    } else { Vec::new() };
    board_cols.retain(|(_, tasks)| !tasks.is_empty());

    // Render board content
    let board_content: Element = if let Some(users)=users_ok.clone() {
        if !list_vec.is_empty() { rsx! { div { class: "flex overflow-x-auto gap-4 p-4 scrollbar-dark h-[calc(100vh-56px)]", tabindex: 0,
            for entry in { board_cols.clone() } { div { class: "flex-none w-[420px] max-w-[440px] h-full", ColumnView { name: entry.0.clone(), tasks: entry.1.clone(), users: users.clone(), on_change: Some(on_change_cb.clone()), on_open: Some(on_open_task.clone()) } } }
        } } } else { rsx! { div { class: "p-6 text-sm opacity-70", "No tasks yet." } } }
    } else if let Some(err) = users_err.clone() { rsx! { div { class: "p-4 text-red-400", {format!("Error loading users: {err}")} } } } else { rsx! { div { class: "p-6 flex items-center justify-center", span { class: "animate-pulse", "Loading..." } } } };

    let users_for_modals: Vec<User> = users_ok.clone().unwrap_or_default();
    rsx! {
        div { class: "h-screen w-screen overflow-hidden bg-[#0b0b0f] text-slate-200 scrollbar-dark flex flex-col", 
            // Header / toolbar
            div { class: "flex items-center gap-3 px-4 h-12 border-b border-[#2a2c5d]/60 bg-[#0f0f14]",
                h1 { class: "text-sm font-semibold tracking-wide", {props.page.clone()} }
                button { class: "ml-auto text-xs px-3 py-1 rounded border border-[#2a2c5d]/70 hover:bg-[#1e1a2a]/60 transition-colors", onclick: move |_| show_create_modal.set(true), "+ New" }
                button { class: "text-xs px-3 py-1 rounded border border-[#2a2c5d]/70 hover:bg-[#1e1a2a]/60", onclick: move |_| { let mut all=all_tasks.to_owned(); spawn(async move { if let Ok(list)=fetch_incomplete_tasks().await { all.set(list); } }); }, "Refresh" }
            }
            div { class: "flex-1 overflow-x-auto overflow-y-hidden", {board_content} }
            if let Some(e)=last_err() { div { class: "fixed bottom-3 right-3 bg-red-900/80 text-red-100 px-3 py-2 rounded shadow", {e} } }
        }
        if show_task_modal() { if let Some(task)=selected_task() { TaskModal { show: show_task_modal, task: task.clone(), users: users_for_modals.clone(), on_change: on_change_cb.clone() } } }
        if show_create_modal() { CreateTaskModal { show: show_create_modal, users: users_for_modals.clone(), on_created: on_change_cb.clone() } }
    }
}

// =========================
// Column View
// =========================
#[derive(Props, PartialEq, Clone)]
struct ColumnViewProps {
    name: String,
    tasks: Vec<LiveTaskPayload>,
    users: Vec<User>,
    #[props(default)] on_change: Option<Callback<LiveTaskPayload>>,
    #[props(default)] on_open: Option<Callback<LiveTaskPayload>>,
}

#[component]
fn ColumnView(props: ColumnViewProps) -> Element {
    let mut query = use_signal(|| String::new());
    let mut show_search = use_signal(|| false);
    let mut collapsed = use_signal(|| false);
    let filtered: Vec<LiveTaskPayload> = if query().trim().is_empty() { props.tasks.clone() } else { let names = props.tasks.iter().map(|t| t.task_name.as_str()); props.tasks.clone().filter_by_task_name(names, query()) };
    rsx! { div { class: "rounded-lg border border-[#2a2c5d]/60 bg-[#0c0c10] flex flex-col h-full",
        div { class: "px-3 py-2 text-xs font-semibold border-b border-[#2a2c5d]/60 flex items-center gap-2",
            span { {format!("{} ({})", props.name.clone(), filtered.len())} }
            button { class: "ml-auto px-2 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#1e1a2a]/50", onclick: move |_| collapsed.set(!collapsed()), { if collapsed() { "▸" } else { "▾" } } }
            button { class: "ml-auto px-2 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#1e1a2a]/50", onclick: move |_| show_search.set(!show_search()), "🔎" }
        }
        if show_search() { div { class: "px-2 pt-2 border-b border-[#2a2c5d]/40", input { class: "w-full bg-[#111216] rounded px-2 py-1 text-xs border border-[#2a2c5d]/60", value: query(), placeholder: "Search in column...", oninput: move |e| query.set(e.value()) } } }
        if !collapsed() { div { class: "p-2 space-y-2 overflow-y-auto flex-1 min-h-0",
            for task in filtered.iter().cloned() { TaskCard { task: task, users: props.users.clone(), on_change: props.on_change.clone(), on_open: props.on_open.clone() } }
        } }
    } }
}

// =========================
// Task Card
// =========================
#[derive(Props, PartialEq, Clone)]
pub struct TaskCardProps { task: LiveTaskPayload, users: Vec<User>, #[props(default)] on_change: Option<Callback<LiveTaskPayload>>, #[props(default)] on_open: Option<Callback<LiveTaskPayload>> }

#[component]
fn TaskCard(props: TaskCardProps) -> Element {
    let mut open_notes = use_signal(|| false);
    let mut open_details = use_signal(|| false);

    let task_for_toggle = props.task.clone();
    let task_for_status = props.task.clone();
    let users_for_assign = props.users.clone();
    let task_for_assign = props.task.clone();
    let completed = props.task.completed;
    let task_name_text = props.task.task_name.clone();
    let status_value = props.task.status.as_str().to_string();

    let priority_color = match props.task.priority.clone() {
        Priority::Express | Priority::Fire => "text-[#ff3766]",
        Priority::Rfs => "text-[#d9ff00]",
        Priority::Qc => "text-[#0bf4c0]",
        Priority::Normal => "text-slate-300",
    };
    let due_class = { let now = Utc::now().date_naive(); let due = props.task.due_date.date_naive(); if due < now { "text-[#ff3766]" } else if due <= now + chrono::Days::new(3) { "text-[#d9ff00]" } else { "text-[#0bf4c0]" } };
    let assignee_name = props.users.iter().find(|u| u.get_id() == props.task.assignee).map(|u| u.get_username().to_string()).unwrap_or_else(|| "Unassigned".into());

    rsx! { div { class: "rounded-lg border border-[#2a2c5d]/60 bg-[#0c0c10] shadow-[0_0_7px_5px_rgba(17,17,41,0.46)]",
        div { class: "flex items-center gap-2 p-3",
            input { r#type: "checkbox", checked: completed, class: "accent-[#0bf4c0]", onchange: move |_| {
                let t = task_for_toggle.clone(); let cb = props.on_change.clone(); spawn(async move { if toggle_completed(&t).await.is_ok() { if let Some(cb)=cb { let mut updated = t.clone(); updated.completed = !updated.completed; cb.call(updated); } } }); }
            }
            div { class: "font-medium flex-1 cursor-pointer hover:underline", onclick: move |_| { if let Some(cb)=&props.on_open { cb.call(props.task.clone()); } }, span { class: "text-sm", {task_name_text.clone()} } }
            button { class: "px-2 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#251d3d]/50", onclick: move |_| open_notes.set(!open_notes()), "💬" }
            button { class: "px-2 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#251d3d]/50", onclick: move |_| open_details.set(!open_details()), "📄" }
        }
        div { class: "px-3 pb-3 text-xs text-slate-300 space-y-2",
            div { class: "flex items-center gap-2", span { class: format!("font-mono {}", priority_color), {props.task.priority.as_str()} } span { class: format!("font-mono ml-auto {}", due_class), {props.task.due_date.format("%m/%d").to_string()} } }
            div { class: "flex items-center gap-2",
                select { class: "bg-[#111116] rounded px-2 py-1 border border-[#2a2c5d]/60 text-slate-200", value: status_value, onchange: move |e| {
                    let t = task_for_status.clone(); let status = Status::from_str(&e.value()); let cb = props.on_change.clone(); spawn(async move { if update_status(&t, status.clone()).await.is_ok() { if let Some(cb)=cb { let mut updated = t.clone(); updated.status = status; cb.call(updated); } } }); },
                    option { value: "Todo", "Todo" } option { value: "In Repair", "In Repair" } option { value: "QC", "QC" } option { value: "Sales", "Sales" } option { value: "Complete", "Complete" }
                }
                select { class: "bg-[#111116] rounded px-2 py-1 border border-[#2a2c5d]/60 text-slate-200 ml-auto", onchange: move |e| {
                    if let Some(u) = users_for_assign.iter().find(|u| u.get_username() == e.value()) { let t = task_for_assign.clone(); let id = u.get_id(); let id2 = id.clone(); let cb = props.on_change.clone(); spawn(async move { if update_assignee(&t, id).await.is_ok() { if let Some(cb)=cb { let mut updated = t.clone(); updated.assignee = id2; cb.call(updated); } } }); }
                }, option { value: assignee_name.clone(), selected: true, {assignee_name.clone()} } for u in props.users.iter().filter(|u| u.is_active()) { option { value: u.get_username(), {u.get_username()} } } }
            }
            if open_details() { DetailsPanel { task: props.task.clone() } }
            if open_notes() { NotesPanel { task: props.task.clone() } }
        }
    } }
}

// =========================
// Details Panel
// =========================
#[derive(Props, PartialEq, Clone)]
pub struct DetailsPanelProps { task: LiveTaskPayload }

#[component]
fn DetailsPanel(props: DetailsPanelProps) -> Element { rsx! { div { class: "mt-2 rounded border border-[#2a2c5d]/40 bg-[#0f1014] p-2 space-y-2",
    div { class: "text-xs font-semibold opacity-70", "Description" }
    div { class: "text-xs whitespace-pre-wrap font-mono", {props.task.task_description.clone()} }
    div { class: "text-xs font-semibold opacity-70 pt-2", "Recent Notes" }
    NotesPanel { task: props.task.clone() }
} } }

// =========================
// Notes Panel
// =========================
#[derive(Props, PartialEq, Clone)]
pub struct NotesPanelProps { task: LiveTaskPayload }

#[component]
fn NotesPanel(props: NotesPanelProps) -> Element {
    let notes = use_resource({ let id_outer = props.task.id.clone(); move || { let id_inner = id_outer.clone(); async move { fetch_task_notes(&id_inner).await } } });
    let mut new_note = use_signal(|| String::new());
    let mut private = use_signal(|| false);
    rsx! { div { class: "mt-2 rounded border border-[#2a2c5d]/40 bg-[#0f1014] p-2",
        match notes.read().as_ref() {
            Some(Ok(list)) if !list.is_empty() => rsx!{ for n in list.iter() { div { class: "text-xs text-slate-200 py-1",
                div { class: "opacity-70", {format!("{} · {}", n.username, n.created_at.format("%m/%d %I:%M%p"))} }
                div { class: "font-mono whitespace-pre-wrap", {n.note.clone()} }
            } } },
            Some(Ok(_)) => rsx!{ div { class: "text-xs opacity-60", "No notes yet." } },
            Some(Err(e)) => rsx!{ div { class: "text-red-400", "{e}" } },
            None => rsx!{ div { class: "text-xs opacity-60", "Loading notes..." } }
        }
        div { class: "flex items-center gap-2 mt-2",
            textarea { class: "flex-1 bg-[#111216] rounded px-2 py-1 text-sm border border-[#2a2c5d]/60", rows: 2, value: new_note(), oninput: move |e| new_note.set(e.value()) }
            label { class: "flex items-center gap-1 text-xs", input { r#type: "checkbox", checked: private(), onchange: move |_| private.set(!private()) } span { "Private" } }
            button { class: "px-3 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#251d3d]/50 text-sm", onclick: move |_| {
                let text = new_note().trim().to_string(); if text.is_empty() { return; } new_note.set(String::new()); let t = props.task.clone(); let priv_flag = private();
                spawn(async move { if let Ok(user) = current_user().await { let _ = add_note(t.id.clone(), &user, text, priv_flag, t.service_number.clone()).await; } });
            }, "Add" }
        }
    } }
}

// =========================
// Task & Create Modals
// =========================
// =========================
// Tabbed Task Modal (mirrors egui multi-page concept)
// =========================
#[derive(Clone, PartialEq)]
enum TaskModalTab { TicketInfo, ComputerInfo, SoftwareInfo, Notes }

impl TaskModalTab { fn label(&self) -> &'static str { match self { TaskModalTab::TicketInfo=>"Ticket", TaskModalTab::ComputerInfo=>"Computer", TaskModalTab::SoftwareInfo=>"Software", TaskModalTab::Notes=>"Notes" } } }

#[derive(Props, PartialEq, Clone)]
struct TaskModalProps { show: Signal<bool>, task: LiveTaskPayload, users: Vec<User>, #[props(default)] on_change: Option<Callback<LiveTaskPayload>> }

#[component]
fn TaskModal(props: TaskModalProps) -> Element {
    // Core editable signals
    let mut tab = use_signal(|| TaskModalTab::TicketInfo);
    let mut name = use_signal(|| props.task.task_name.clone());
    let mut desc = use_signal(|| props.task.task_description.clone());
    let mut status = use_signal(|| props.task.status.as_str().to_string());
    let mut assignee = use_signal(|| props.users.iter().find(|u| u.get_id()==props.task.assignee).map(|u| u.get_username().to_string()).unwrap_or_else(|| "Unassigned".into()));

    // Loaded associated data
    let ticket_sig = use_signal(|| Option::<TicketData>::None);
    let computer_sig = use_signal(|| Option::<ComputerData>::None);
    let customer_sig = use_signal(|| Option::<CustomerData>::None);

    // Fetch associated objects once
    {
        let task_id_outer = props.task.id.clone();
        let mut t_sig = ticket_sig.to_owned();
        let mut c_sig = computer_sig.to_owned();
        let mut cust_sig = customer_sig.to_owned();
        use_effect(move || {
            let task_id_clone = task_id_outer.clone();
            spawn(async move {
                if let Ok(t) = TicketData::get_associated_ticket(task_id_clone.clone()).await { t_sig.set(Some(t)); }
                if let Ok(c) = ComputerData::get_associated_computer(task_id_clone.clone()).await { c_sig.set(Some(c)); }
                if let Ok(cu) = CustomerData::get_associated_customer(task_id_clone.clone()).await { cust_sig.set(Some(cu)); }
            });
        });
    }

    let users = props.users.clone();
    let t_for_status = props.task.clone();
    let t_for_assign = props.task.clone();

    // Shared update closure
    let save_changes = {
        use std::rc::Rc;
        let mut show = props.show.to_owned();
        let cb = props.on_change.clone();
        let orig = props.task.clone();
        Rc::new(move |new_name:String, new_desc:String| {
            let orig_cloned = orig.clone();
            let cb2 = cb.clone();
            let mut show2 = show.to_owned();
            spawn(async move {
                let mut task = orig_cloned.clone();
                if new_name!=task.task_name { let _ = task.update_task_name(new_name.clone()).await; task.task_name=new_name; }
                if new_desc!=task.task_description { task.task_description=new_desc.clone(); let _ = task.update_task_description().await; }
                if let Some(cb)=cb2 { cb.call(task.clone()); }
                show2.set(false);
            });
        })
    };

    // Render tab content
    let ticket_view: Element = rsx! { div { class: "space-y-3", 
        div { class: "grid grid-cols-2 gap-2 text-xs", 
            if let Some(t) = ticket_sig() { 
                div { class: "col-span-1 opacity-70", "Service #" } div { class: "col-span-1", {t.service_number.clone()} }
                div { class: "col-span-1 opacity-70", "Sales Rep" } div { class: "col-span-1", {t.sales_rep.clone()} }
            } else { div { class: "text-xs opacity-60", "No ticket loaded" } }
        }
        textarea { class: "w-full bg-[#111216] rounded px-3 py-2 border border-[#2a2c5d]/60", rows: 4, value: desc(), oninput: move |e| desc.set(e.value()) }
    } };

    let computer_view: Element = rsx! { div { class: "space-y-2 text-xs", 
        match computer_sig() { Some(c) => rsx!{ div { class: "grid grid-cols-2 gap-x-3 gap-y-1", 
            div { class: "opacity-60", "Hostname" } div { {c.hostname.clone()} }
            div { class: "opacity-60", "CPU" } div { {c.cpu.clone()} }
            div { class: "opacity-60", "GPU" } div { {c.gpu.clone()} }
            div { class: "opacity-60", "RAM" } div { {c.ram.clone()} }
        } }, None => rsx!{ div { class: "opacity-60", "No computer info" } } }
    } };

    let software_view: Element = rsx! { div { class: "space-y-2 text-xs", match computer_sig() { Some(c) => rsx!{ if !c.current_antivirus.is_empty() { div { class: "font-semibold text-xs", "Antivirus" } ul { class: "list-disc ml-4 space-y-1", for av in c.current_antivirus.iter() { li { {av.clone()} } } } } else { div { class: "opacity-60", "No software data" } } }, None => rsx!{ div { class: "opacity-60", "No software data" } } } } };

    let notes_view: Element = rsx! { NotesPanel { task: props.task.clone() } };

    rsx! { crate::components::dialog::Dialog { show_modal: props.show, wrap_class: Some("w-[97%] max-w-4xl".into()),
        div { class: "text-slate-200 space-y-4",
            // Header row
            div { class: "flex items-center gap-3", 
                input { class: "flex-1 bg-[#111216] rounded px-3 py-2 border border-[#2a2c5d]/60", value: name(), oninput: move |e| name.set(e.value()) }
                select { class: "bg-[#111216] rounded px-2 py-1 border border-[#2a2c5d]/60 text-xs", value: status(), oninput: move |e| { status.set(e.value()); let t=t_for_status.clone(); let cb=props.on_change.clone(); let st=Status::from_str(&e.value()); spawn(async move { if update_status(&t, st.clone()).await.is_ok() { if let Some(cb)=cb { let mut u=t.clone(); u.status=st; cb.call(u);} } }); }, option { value: "Todo", "Todo" } option { value: "In Repair", "In Repair" } option { value: "QC", "QC" } option { value: "Sales", "Sales" } option { value: "Complete", "Complete" } }
                select { class: "bg-[#111216] rounded px-2 py-1 border border-[#2a2c5d]/60 text-xs", value: assignee(), oninput: move |e| { if let Some(u)=users.iter().find(|u| u.get_username()==e.value()) { let t=t_for_assign.clone(); let id=u.get_id(); let id2=id.clone(); let cb=props.on_change.clone(); spawn(async move { if update_assignee(&t,id).await.is_ok() { if let Some(cb)=cb { let mut u2=t.clone(); u2.assignee=id2; cb.call(u2);} } }); assignee.set(e.value()); } }, option { value: assignee(), selected: true, {assignee()} } for u in users.iter().filter(|u| u.is_active()) { option { value: u.get_username(), {u.get_username()} } } }
            }
            // Tabs
            div { class: "flex gap-2 border-b border-[#2a2c5d]/60 text-xs",
                // Ticket Tab
                { let current = tab(); rsx!{ button { class: format!("px-3 py-1 rounded-t border border-b-0 {}", if current==TaskModalTab::TicketInfo { "bg-[#1b1d28] border-[#2a2c5d]" } else { "border-transparent hover:bg-[#1b1d28]/40" }), onclick: move |_| tab.set(TaskModalTab::TicketInfo), "Ticket" } } }
                { let current = tab(); rsx!{ button { class: format!("px-3 py-1 rounded-t border border-b-0 {}", if current==TaskModalTab::ComputerInfo { "bg-[#1b1d28] border-[#2a2c5d]" } else { "border-transparent hover:bg-[#1b1d28]/40" }), onclick: move |_| tab.set(TaskModalTab::ComputerInfo), "Computer" } } }
                { let current = tab(); rsx!{ button { class: format!("px-3 py-1 rounded-t border border-b-0 {}", if current==TaskModalTab::SoftwareInfo { "bg-[#1b1d28] border-[#2a2c5d]" } else { "border-transparent hover:bg-[#1b1d28]/40" }), onclick: move |_| tab.set(TaskModalTab::SoftwareInfo), "Software" } } }
                { let current = tab(); rsx!{ button { class: format!("px-3 py-1 rounded-t border border-b-0 {}", if current==TaskModalTab::Notes { "bg-[#1b1d28] border-[#2a2c5d]" } else { "border-transparent hover:bg-[#1b1d28]/40" }), onclick: move |_| tab.set(TaskModalTab::Notes), "Notes" } } }
            }
            div { class: "min-h-[240px] text-sm", match tab() { TaskModalTab::TicketInfo => ticket_view, TaskModalTab::ComputerInfo => computer_view, TaskModalTab::SoftwareInfo => software_view, TaskModalTab::Notes => notes_view } }
            div { class: "text-right", button { class: "px-3 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#1e1a2a]/50 text-xs", onclick: move |_| { (save_changes.clone())(name(), desc()); }, "Save & Close" } }
        }
    } }
}

#[derive(Props, PartialEq, Clone)]
struct CreateTaskModalProps { show: Signal<bool>, users: Vec<User>, #[props(default)] on_created: Option<Callback<LiveTaskPayload>> }

#[component]
fn CreateTaskModal(props: CreateTaskModalProps) -> Element {
    // Basic fields
    let mut service = use_signal(|| String::new());
    let mut name = use_signal(|| String::new());
    let mut desc = use_signal(|| String::new());
    let mut prio = use_signal(|| "Normal".to_string());
    let mut assignee = use_signal(|| props.users.first().map(|u| u.get_username().to_string()).unwrap_or_default());
    let mut due = use_signal(|| {
        let now = Utc::now().date_naive();
        now.format("%Y-%m-%d").to_string()
    });
    // TUR / Prestashop payload
    let tur_payload = use_signal(|| Option::<PrestashopPayload>::None);
    let mut pulling = use_signal(|| false);

    // Auto-fill task name from payload (CustomerData.name is already combined)
    {
        let mut nm = name.to_owned();
        let service_sig = service.to_owned();
        use_effect(move || { if let Some(p)=tur_payload() { if nm().is_empty() { nm.set(format!("{} - {}", p.customer.name, service_sig())); } } });
    }

    let users = props.users.clone();
    let can_submit = move || !name().trim().is_empty() && !desc().trim().is_empty() && !assignee().trim().is_empty();

    rsx! { crate::components::dialog::Dialog { show_modal: props.show, wrap_class: Some("w-[98%] max-w-2xl".into()),
        div { class: "text-slate-200 space-y-4 text-sm",
            div { class: "flex items-center gap-2", input { class: "flex-1 bg-[#111216] rounded px-3 py-2 border border-[#2a2c5d]/60", placeholder: "Service #", value: service(), oninput: move |e| service.set(e.value()) }
                button { class: "px-3 py-1 rounded border border-[#2a2c5d]/60 disabled:opacity-40", disabled: pulling(), onclick: move |_| { if service().trim().is_empty() { return; } pulling.set(true); let svc = service(); let mut tp = tur_payload.to_owned(); spawn(async move { let res = get_prestashop_payload(&svc).await; if let Ok(p) = res { tp.set(Some(p)); } pulling.set(false); }); }, { if pulling() { "Loading..." } else { "Pull Order" } } }
            }
            if let Some(p)=tur_payload() { div { class: "grid grid-cols-3 gap-x-4 gap-y-1 text-xs bg-[#111216] p-3 rounded border border-[#2a2c5d]/40", 
                div { class: "col-span-1 opacity-60", "Customer" } div { class: "col-span-2", {p.customer.name.clone()} }
                div { class: "col-span-1 opacity-60", "Email" } div { class: "col-span-2 break-all", {p.customer.email.clone()} }
                div { class: "col-span-1 opacity-60", "Order ID" } div { class: "col-span-2", {p.order.id.clone()} }
                div { class: "col-span-1 opacity-60", "Address" } div { class: "col-span-2", {format!("{} {} {}", p.address.address1, p.address.city, p.address.postcode)} }
                if let Some(rep)=p.sales_rep.clone() { div { class: "col-span-1 opacity-60", "Rep" } div { class: "col-span-2", {format!("{} {}", rep.firstname, rep.lastname)} } }
            } }
            input { class: "w-full bg-[#111216] rounded px-3 py-2 border border-[#2a2c5d]/60", placeholder: "Task name", value: name(), oninput: move |e| name.set(e.value()) }
            textarea { class: "w-full bg-[#111216] rounded px-3 py-2 border border-[#2a2c5d]/60", rows: 4, placeholder: "Description", value: desc(), oninput: move |e| desc.set(e.value()) }
            div { class: "grid grid-cols-3 gap-2", 
                div { class: "flex flex-col gap-1", label { class: "text-[10px] uppercase tracking-wide opacity-70", "Priority" } select { class: "bg-[#111216] rounded px-2 py-1 border border-[#2a2c5d]/60", value: prio(), oninput: move |e| prio.set(e.value()), for p in Priority::VALUES { option { value: p.as_str(), {p.as_str()} } } } }
                div { class: "flex flex-col gap-1", label { class: "text-[10px] uppercase tracking-wide opacity-70", "Assignee" } select { class: "bg-[#111216] rounded px-2 py-1 border border-[#2a2c5d]/60", value: assignee(), oninput: move |e| assignee.set(e.value()), for u in users.iter().filter(|u| u.is_active()) { option { value: u.get_username(), {u.get_username()} } } } }
                div { class: "flex flex-col gap-1", label { class: "text-[10px] uppercase tracking-wide opacity-70", "Due Date" } input { r#type: "date", class: "bg-[#111216] rounded px-2 py-1 border border-[#2a2c5d]/60 w-full", value: due(), oninput: move |e| due.set(e.value()) } }
            }
            div { class: "text-right", button { class: "px-4 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#1e1a2a]/50 disabled:opacity-40", disabled: !can_submit(), onclick: move |_| {
                if !can_submit() { return; }
                let svc_opt = if service().trim().is_empty() { None } else { Some(service()) };
                let payload = NewTaskInput { task_name: name(), task_description: desc(), service_number: svc_opt, priority: parse_priority(&prio()), assignee_username: assignee() };
                let cb = props.on_created.clone(); let mut show = props.show.to_owned();
                spawn(async move { if let Ok(mut created) = create_task_simple(payload).await { // if we pulled order & want full create (emulating egui) just best-effort
                        if let Some(cb)=cb { cb.call(created.clone()); }
                    } show.set(false); });
            }, "Create Task" } }
        }
    } }
}

fn parse_priority(s: &str) -> Priority { match s { "Express"=>Priority::Express, "Rfs"=>Priority::Rfs, "Fire"=>Priority::Fire, "Qc"=>Priority::Qc, _=>Priority::Normal } }

// =========================
// Helpers
// =========================
async fn current_user() -> anyhow::Result<User> {
    if let Ok(guard) = database::CURRENT_USER_INFO.try_lock() { if let Some(u) = guard.clone() { return Ok(u); } }
    let user: Option<User> = database::DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
    user.ok_or_else(|| anyhow::anyhow!("No current user"))
}
