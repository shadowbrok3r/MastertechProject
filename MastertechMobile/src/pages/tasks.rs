use dioxus::prelude::*;
use chrono::Utc;
use crossbeam_channel::unbounded;
use database::live_data::{listen_data, handle_live_data};
use database::schema::{LiveTaskPayload, User, Priority, Status, TicketData, ComputerData, CustomerData};
use database::schema::utilities::get_prestashop_payload;
use database::schema::prestashop_schema::PrestashopPayload;
use database::schema::task::filter::FilterLiveTasks;
use crate::services::tasks::{
    fetch_incomplete_tasks, fetch_completed_tasks,
    fetch_task_notes, fetch_store_users, toggle_completed, update_status, update_assignee, add_note,
    NewTaskInput, create_task_simple,
};
use crate::components::badge::{Badge, BadgeVariant};
use crate::components::card::{Card, CardContent, CardFooter, CardHeader, CardTitle};
use crate::components::tabs::{TabContent, TabList, Tabs, TabTrigger, TabsVariant};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Task Board (root)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#[derive(Props, PartialEq, Clone)]
pub struct TaskBoardProps {
    pub page: String,
    #[props(default)] pub on_navigate: Option<Callback<String>>,
    #[props(default)] pub refresh_token: u64,
    #[props(default)] pub create_task_trigger: u64,
}

#[component]
pub fn TaskBoard(props: TaskBoardProps) -> Element {
    let all_tasks = use_signal(|| Vec::<LiveTaskPayload>::new());
    let last_err = use_signal(|| Option::<String>::None);
    let current_user_res = use_resource(|| async move { current_user().await });
    let store_users = use_resource(|| async move { Ok::<_, anyhow::Error>(fetch_store_users().await) });
    let mut search = use_signal(|| String::new());

    // Fetch tasks on page / refresh change
    {
        let page = props.page.clone();
        let refresh = props.refresh_token;
        let mut all_sig = all_tasks.to_owned();
        let mut err_sig = last_err.to_owned();
        use_future(move || {
            let page_clone = page.clone();
            async move {
                let res = if page_clone == "Completed Tasks" { fetch_completed_tasks().await } else { fetch_incomplete_tasks().await };
                match res { Ok(list) => { err_sig.set(None); all_sig.set(list); } Err(e) => err_sig.set(Some(e.to_string())) }
            }
        });
        let _ = refresh;
    }

    // Live updates
    {
        let mut all_sig = all_tasks.to_owned();
        use_effect(move || {
            let (tx, rx) = unbounded::<(database::live_data::Action, LiveTaskPayload)>();
            spawn(async move { let _ = listen_data::<LiveTaskPayload>(tx, "task").await; });
            spawn(async move {
                loop {
                    while let Ok(msg) = rx.try_recv() {
                        let mut v = all_sig();
                        let _ = handle_live_data(msg, &mut v);
                        all_sig.set(v);
                    }
                    #[cfg(target_arch = "wasm32")]
                    { gloo_timers::future::TimeoutFuture::new(120).await; }
                    #[cfg(not(target_arch = "wasm32"))]
                    { futures_timer::Delay::new(std::time::Duration::from_millis(120)).await; }
                }
            });
        });
    }

    // Modals
    let show_task_modal = use_signal(|| false);
    let selected_task = use_signal(|| Option::<LiveTaskPayload>::None);
    let mut show_create_modal = use_signal(|| false);
    {
        let trigger = props.create_task_trigger;
        let mut show = show_create_modal.to_owned();
        let mut last_seen = use_signal(|| 0u64);
        use_effect(move || { if trigger > 0 && trigger != last_seen() { last_seen.set(trigger); show.set(true); } });
    }
    let on_open_task = { let mut sel = selected_task.to_owned(); let mut show = show_task_modal.to_owned(); Callback::new(move |t: LiveTaskPayload| { sel.set(Some(t)); show.set(true); }) };
    let on_change_cb = { let mut all = all_tasks.to_owned(); Callback::new(move |updated: LiveTaskPayload| { let mut v = all(); if let Some(i)=v.iter().position(|x| x.id==updated.id){ v[i]=updated; } else { v.push(updated); } all.set(v); }) };

    // Build filtered task list
    let list_vec = all_tasks();
    let user_opt = current_user_res.read().as_ref().and_then(|r| r.as_ref().ok().cloned());
    let (users_ok, users_err) = match store_users.read().as_ref() {
        Some(Ok(u)) => (Some(u.clone()), None),
        Some(Err(e)) => (None, Some(e.to_string())),
        None => (None, None),
    };

    let filtered: Vec<LiveTaskPayload> = if let Some(users) = users_ok.clone() {
        let my_store = user_opt.as_ref().map(|u| u.get_store());
        list_vec.iter().cloned().filter(|t| {
            // Page filter
            match props.page.as_str() {
                "My Tasks" => {
                    let owner_ok = if let Some(u) = &user_opt { t.assignee == u.get_id() } else { true };
                    owner_ok && !t.completed
                }
                "Completed Tasks" => t.completed,
                "Store Tasks" => {
                    !t.completed && {
                        if let Some(s) = my_store {
                            users.iter().find(|u| u.get_id() == t.assignee).map(|u| u.get_store() == s).unwrap_or(true)
                        } else { true }
                    }
                }
                _ => true,
            }
        }).filter(|t| {
            // Search filter
            let q = search().to_lowercase();
            q.is_empty() || t.task_name.to_lowercase().contains(&q) || t.task_description.to_lowercase().contains(&q)
        }).collect()
    } else { Vec::new() };

    let users_for_modals: Vec<User> = users_ok.clone().unwrap_or_default();

    rsx! {
        // Search bar
        div { class: "px-4 pt-3 pb-2",
            input {
                class: "w-full text-xs",
                r#type: "search",
                placeholder: "Search tasks...",
                value: search(),
                oninput: move |e| search.set(e.value()),
            }
        }

        // Task list — horizontal margin so cards don't full-bleed
        if users_ok.is_some() {
            if !filtered.is_empty() {
                div { class: "px-4 pb-4 space-y-3 max-w-2xl mx-auto",
                    for task in filtered.iter().cloned() {
                        TaskCard {
                            task: task,
                            users: users_for_modals.clone(),
                            on_change: Some(on_change_cb.clone()),
                            on_open: Some(on_open_task.clone()),
                        }
                    }
                }
            } else {
                div { class: "p-6 text-center text-sm text-stardust px-4", "No tasks." }
            }
        } else if let Some(err) = users_err.clone() {
            div { class: "p-4 px-4 text-warning-red text-xs", {format!("Error: {err}")} }
        } else {
            div { class: "p-6 text-center text-stardust animate-pulse-glow px-4", "Loading..." }
        }

        if let Some(e) = last_err() {
            div { class: "px-4 pb-2 text-xs text-warning-red", {e} }
        }

        // Modals
        if show_task_modal() {
            if let Some(task) = selected_task() {
                TaskModal { show: show_task_modal, task: task.clone(), users: users_for_modals.clone(), on_change: on_change_cb.clone() }
            }
        }
        if show_create_modal() {
            CreateTaskModal { show: show_create_modal, users: users_for_modals.clone(), on_created: on_change_cb.clone() }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Task Card — compact mobile card
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#[derive(Props, PartialEq, Clone)]
pub struct TaskCardProps {
    task: LiveTaskPayload,
    users: Vec<User>,
    #[props(default)] on_change: Option<Callback<LiveTaskPayload>>,
    #[props(default)] on_open: Option<Callback<LiveTaskPayload>>,
}

#[component]
fn TaskCard(props: TaskCardProps) -> Element {
    let task = &props.task;
    let task_name = task.task_name.clone();
    let status_val = task.status.as_str().to_string();
    let task_for_status = task.clone();

    let priority_variant = match task.priority {
        Priority::Express | Priority::Fire => BadgeVariant::Destructive,
        Priority::Rfs => BadgeVariant::Secondary,
        _ => BadgeVariant::Outline,
    };

    let due_class = {
        let now = Utc::now().date_naive();
        let due = task.due_date.date_naive();
        if due < now { "text-warning-red" }
        else if due <= now + chrono::Days::new(3) { "text-comet-gold" }
        else { "text-alien-green" }
    };

    let assignee_name = props.users.iter().find(|u| u.get_id() == task.assignee)
        .map(|u| u.get_username().to_string()).unwrap_or_else(|| "—".into());

    let card_el: Element = rsx! {
        div { class: "card-cosmic p-4 active:scale-[0.98] transition-transform",
            // Row 1: task name as button + priority badge (no complete checkbox — complete in modal)
            div { class: "flex items-center gap-2",
                button {
                    class: "flex-1 min-w-0 text-left btn-cosmic py-2 px-3 font-medium text-sm truncate",
                    r#type: "button",
                    onclick: move |_| { if let Some(cb) = &props.on_open { cb.call(props.task.clone()); } },
                    {task_name.clone()}
                }
                Badge { variant: priority_variant, span { class: "text-[10px]", {task.priority.as_str()} } }
            }
            // Row 2: status, assignee, due
            div { class: "flex items-center gap-2 mt-3 text-xs",
                select {
                    class: "flex-1 min-w-0 text-xs py-1",
                    value: status_val,
                    onchange: move |e| {
                        let t = task_for_status.clone();
                        let status = Status::from_str(&e.value());
                        let cb = props.on_change.clone();
                        spawn(async move {
                            if update_status(&t, status.clone()).await.is_ok() {
                                if let Some(cb) = cb { let mut u = t.clone(); u.status = status; cb.call(u); }
                            }
                        });
                    },
                    option { value: "Todo", "Todo" }
                    option { value: "In Repair", "In Repair" }
                    option { value: "QC", "QC" }
                    option { value: "Sales", "Sales" }
                    option { value: "Complete", "Complete" }
                }
                span { class: "text-stardust truncate max-w-[80px]", {assignee_name} }
                span { class: format!("font-mono flex-shrink-0 {}", due_class), {task.due_date.format("%m/%d").to_string()} }
            }
        }
    };
    card_el
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Notes Panel
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#[derive(Props, PartialEq, Clone)]
pub struct NotesPanelProps { task: LiveTaskPayload }

#[component]
fn NotesPanel(props: NotesPanelProps) -> Element {
    let notes = use_resource({ let id = props.task.id.clone(); move || { let id = id.clone(); async move { fetch_task_notes(&id).await } } });
    let mut new_note = use_signal(|| String::new());
    let mut private = use_signal(|| false);

    rsx! {
        div { class: "space-y-2",
            match notes.read().as_ref() {
                Some(Ok(list)) if !list.is_empty() => rsx!{
                    for n in list.iter() {
                        div { class: "text-xs text-moonlight py-1 border-b border-[#401c2a]/20 last:border-0",
                            div { class: "text-stardust text-[10px]", {format!("{} · {}", n.username, n.created_at.format("%m/%d %I:%M%p"))} }
                            div { class: "whitespace-pre-wrap mt-0.5", {n.note.clone()} }
                        }
                    }
                },
                Some(Ok(_)) => rsx!{ div { class: "text-xs text-stardust", "No notes yet." } },
                Some(Err(e)) => rsx!{ div { class: "text-xs text-warning-red", "{e}" } },
                None => rsx!{ div { class: "text-xs text-stardust", "Loading..." } }
            }
            div { class: "flex items-end gap-2",
                textarea { class: "flex-1 text-xs", rows: 2, placeholder: "Add a note...", value: new_note(), oninput: move |e| new_note.set(e.value()) }
                div { class: "flex flex-col gap-1 items-center",
                    label { class: "flex items-center gap-1 text-[10px] text-stardust",
                        input { r#type: "checkbox", checked: private(), onchange: move |_| private.set(!private()) }
                        "Priv"
                    }
                    button { class: "btn-nebula text-xs px-3 py-1", onclick: move |_| {
                        let text = new_note().trim().to_string();
                        if text.is_empty() { return; }
                        new_note.set(String::new());
                        let t = props.task.clone();
                        let priv_flag = private();
                        spawn(async move {
                            if let Ok(user) = current_user().await {
                                let _ = add_note(t.id.clone(), &user, text, priv_flag, t.service_number.clone()).await;
                            }
                        });
                    }, "Send" }
                }
            }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Task Detail Modal — Card + Tabs, complete only inside modal
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#[derive(Props, PartialEq, Clone)]
struct TaskModalProps { show: Signal<bool>, task: LiveTaskPayload, users: Vec<User>, #[props(default)] on_change: Option<Callback<LiveTaskPayload>> }

#[component]
fn TaskModal(props: TaskModalProps) -> Element {
    let mut tab_value = use_signal(|| Some("details".to_string()));
    let mut name = use_signal(|| props.task.task_name.clone());
    let mut desc = use_signal(|| props.task.task_description.clone());
    let mut status = use_signal(|| props.task.status.as_str().to_string());
    let mut assignee_name = use_signal(|| props.users.iter().find(|u| u.get_id() == props.task.assignee).map(|u| u.get_username().to_string()).unwrap_or_default());
    let mut completed = use_signal(|| props.task.completed);

    let ticket_sig = use_signal(|| Option::<TicketData>::None);
    let computer_sig = use_signal(|| Option::<ComputerData>::None);

    {
        let task_id = props.task.id.clone();
        let mut t_sig = ticket_sig.to_owned();
        let mut c_sig = computer_sig.to_owned();
        use_effect(move || {
            let id = task_id.clone();
            spawn(async move {
                if let Ok(t) = TicketData::get_associated_ticket(id.clone()).await { t_sig.set(Some(t)); }
                if let Ok(c) = ComputerData::get_associated_computer(id.clone()).await { c_sig.set(Some(c)); }
            });
        });
    }

    let users = props.users.clone();
    let t_for_status = props.task.clone();
    let t_for_assign = props.task.clone();
    let task_for_toggle = props.task.clone();

    let save_changes = {
        use std::rc::Rc;
        let mut show = props.show.to_owned();
        let cb = props.on_change.clone();
        let orig = props.task.clone();
        Rc::new(move |new_name: String, new_desc: String| {
            let orig = orig.clone(); let cb2 = cb.clone(); let mut show2 = show.to_owned();
            spawn(async move {
                let mut task = orig.clone();
                if new_name != task.task_name { let _ = task.update_task_name(new_name.clone()).await; task.task_name = new_name; }
                if new_desc != task.task_description { task.task_description = new_desc.clone(); let _ = task.update_task_description().await; }
                if let Some(cb) = cb2 { cb.call(task); }
                show2.set(false);
            });
        })
    };

    let on_tab_change = Callback::new(move |s: String| { tab_value.set(Some(s)); });
    let disabled_sig = use_signal(|| false);
    let horizontal_sig = use_signal(|| true);
    let idx0 = use_signal(|| 0usize);
    let idx1 = use_signal(|| 1usize);
    let idx2 = use_signal(|| 2usize);

    // Details tab: name, status, assignee, description, ticket info, and Complete checkbox
    let details_el: Element = rsx! {
        div { class: "space-y-3 text-star-white",
            label { class: "flex items-center gap-2 text-sm",
                input {
                    r#type: "checkbox",
                    checked: completed(),
                    onchange: move |_| {
                        let t = task_for_toggle.clone();
                        let cb = props.on_change.clone();
                        let mut comp = completed.to_owned();
                        spawn(async move {
                            if toggle_completed(&t).await.is_ok() {
                                comp.set(!comp());
                                if let Some(cb) = cb { let mut u = t.clone(); u.completed = !u.completed; cb.call(u); }
                            }
                        });
                    }
                }
                span { "Mark complete" }
            }
            input { class: "w-full text-sm font-medium", value: name(), oninput: move |e| name.set(e.value()) }
            div { class: "flex gap-2",
                select { class: "flex-1 text-xs", value: status(), onchange: move |e| {
                    status.set(e.value());
                    let t = t_for_status.clone(); let st = Status::from_str(&e.value()); let cb = props.on_change.clone();
                    spawn(async move { if update_status(&t, st.clone()).await.is_ok() { if let Some(cb)=cb { let mut u=t.clone(); u.status=st; cb.call(u);} } });
                },
                    option { value: "Todo", "Todo" } option { value: "In Repair", "In Repair" }
                    option { value: "QC", "QC" } option { value: "Sales", "Sales" } option { value: "Complete", "Complete" }
                }
                select { class: "flex-1 text-xs", value: assignee_name(), onchange: move |e| {
                    if let Some(u) = users.iter().find(|u| u.get_username() == e.value()) {
                        let t = t_for_assign.clone(); let id = u.get_id(); let id2 = id.clone(); let cb = props.on_change.clone();
                        spawn(async move { if update_assignee(&t, id).await.is_ok() { if let Some(cb)=cb { let mut u2=t.clone(); u2.assignee=id2; cb.call(u2);} } });
                        assignee_name.set(e.value());
                    }
                },
                    option { value: assignee_name(), selected: true, {assignee_name()} }
                    for u in props.users.iter().filter(|u| u.is_active()) { option { value: u.get_username(), {u.get_username()} } }
                }
            }
            textarea { class: "w-full text-xs", rows: 5, value: desc(), oninput: move |e| desc.set(e.value()) }
            if let Some(t) = ticket_sig() {
                div { class: "text-xs space-y-1 text-stardust",
                    div { class: "flex gap-2", span { "Service #" } span { class: "text-moonlight", {t.service_number.clone()} } }
                    div { class: "flex gap-2", span { "Sales Rep" } span { class: "text-moonlight", {t.sales_rep.clone()} } }
                }
            }
        }
    };

    let computer_el: Element = rsx! {
        div { class: "space-y-2 text-xs text-star-white",
            match computer_sig() {
                Some(c) => rsx! {
                    div { class: "grid grid-cols-2 gap-x-3 gap-y-1",
                        div { class: "text-stardust", "Hostname" } div { class: "text-moonlight", {c.hostname.clone()} }
                        div { class: "text-stardust", "CPU" } div { class: "text-moonlight", {c.cpu.clone()} }
                        div { class: "text-stardust", "GPU" } div { class: "text-moonlight", {c.gpu.clone()} }
                        div { class: "text-stardust", "RAM" } div { class: "text-moonlight", {c.ram.clone()} }
                    }
                    if !c.current_antivirus.is_empty() {
                        div { class: "mt-2",
                            div { class: "text-stardust font-semibold", "Antivirus" }
                            ul { class: "list-disc ml-4 text-moonlight",
                                // `current_antivirus` was `Vec<String>`,
                                // now `Vec<InstalledSecurityProduct>`.
                                // Mobile UI keeps it terse — just the
                                // name, optionally with version and an
                                // Active / Disabled indicator.
                                for product in c.current_antivirus.iter() {
                                    li {
                                        {match product.version.as_deref() {
                                            Some(v) => format!("{} {}", product.name, v),
                                            None => product.name.clone(),
                                        }}
                                        {match product.active {
                                            Some(true) => " (Active)",
                                            Some(false) => " (Disabled)",
                                            None => "",
                                        }}
                                    }
                                }
                            }
                        }
                    }
                },
                None => rsx! { div { class: "text-stardust", "No computer info" } }
            }
        }
    };

    let notes_el: Element = rsx! { NotesPanel { task: props.task.clone() } };

    let card_content: Element = rsx! {
        Card { class: "card-cosmic border-0 w-full max-w-lg".to_string(),
            CardHeader {
                CardTitle { class: "text-base text-star-white", {props.task.task_name.clone()} }
            }
            CardContent { class: "text-star-white",
                Tabs {
                    value: tab_value,
                    default_value: "details".to_string(),
                    on_value_change: on_tab_change,
                    disabled: disabled_sig,
                    horizontal: horizontal_sig,
                    variant: TabsVariant::Default,
                    TabList {
                        TabTrigger { value: "details", index: idx0, "Details" }
                        TabTrigger { value: "computer", index: idx1, "Computer" }
                        TabTrigger { value: "notes", index: idx2, "Notes" }
                    }
                    TabContent { value: "details", index: idx0, children: details_el }
                    TabContent { value: "computer", index: idx1, children: computer_el }
                    TabContent { value: "notes", index: idx2, children: notes_el }
                }
            }
            CardFooter { class: "flex justify-end gap-2",
                button { class: "btn-cosmic text-xs", onclick: move |_| { let mut s = props.show.to_owned(); s.set(false); }, "Close" }
                button { class: "btn-nebula text-xs", onclick: move |_| { (save_changes.clone())(name(), desc()); }, "Save" }
            }
        }
    };
    rsx! { crate::components::modal::Dialog { show_modal: props.show, wrap_class: Some("w-[95%] max-w-lg".into()), children: card_content } }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Create Task Modal
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#[derive(Props, PartialEq, Clone)]
struct CreateTaskModalProps { show: Signal<bool>, users: Vec<User>, #[props(default)] on_created: Option<Callback<LiveTaskPayload>> }

#[component]
fn CreateTaskModal(props: CreateTaskModalProps) -> Element {
    let mut service = use_signal(|| String::new());
    let mut name = use_signal(|| String::new());
    let mut desc = use_signal(|| String::new());
    let mut prio = use_signal(|| "Normal".to_string());
    let mut assignee = use_signal(|| props.users.first().map(|u| u.get_username().to_string()).unwrap_or_default());
    let tur_payload = use_signal(|| Option::<PrestashopPayload>::None);
    let mut pulling = use_signal(|| false);

    {
        let mut nm = name.to_owned();
        let service_sig = service.to_owned();
        use_effect(move || { if let Some(p) = tur_payload() { if nm().is_empty() { nm.set(format!("{} - {}", p.customer.name, service_sig())); } } });
    }

    let users = props.users.clone();
    let can_submit = move || !name().trim().is_empty() && !desc().trim().is_empty() && !assignee().trim().is_empty();

    let body: Element = rsx! {
        div { class: "text-star-white space-y-3 text-sm",
            h3 { class: "text-base font-semibold", "New Task" }
            div { class: "flex gap-2",
                input { class: "flex-1 text-xs", placeholder: "Service #", value: service(), oninput: move |e| service.set(e.value()) }
                button {
                    class: "btn-cosmic text-xs disabled:opacity-40",
                    disabled: pulling(),
                    onclick: move |_| {
                        if service().trim().is_empty() { return; }
                        pulling.set(true);
                        let svc = service();
                        let mut tp = tur_payload.to_owned();
                        spawn(async move {
                            let res = get_prestashop_payload(&svc).await;
                            if let Ok(p) = res { tp.set(Some(p)); }
                            pulling.set(false);
                        });
                    },
                    { if pulling() { "..." } else { "Pull" } }
                }
            }
            if let Some(p) = tur_payload() {
                div { class: "text-xs card-stat p-2 space-y-0.5",
                    div { class: "flex gap-2", span { class: "text-stardust w-16", "Customer" } span { class: "text-moonlight", {p.customer.name.clone()} } }
                    div { class: "flex gap-2", span { class: "text-stardust w-16", "Email" } span { class: "text-moonlight truncate", {p.customer.email.clone()} } }
                    div { class: "flex gap-2", span { class: "text-stardust w-16", "Order" } span { class: "text-moonlight", {p.order.id.clone()} } }
                }
            }
            input { class: "w-full text-xs", placeholder: "Task name", value: name(), oninput: move |e| name.set(e.value()) }
            textarea { class: "w-full text-xs", rows: 3, placeholder: "Description", value: desc(), oninput: move |e| desc.set(e.value()) }
            div { class: "flex gap-2",
                select { class: "flex-1 text-xs", value: prio(), oninput: move |e| prio.set(e.value()),
                    for p in Priority::VALUES { option { value: p.as_str(), {p.as_str()} } }
                }
                select { class: "flex-1 text-xs", value: assignee(), oninput: move |e| assignee.set(e.value()),
                    for u in users.iter().filter(|u| u.is_active()) { option { value: u.get_username(), {u.get_username()} } }
                }
            }
            div { class: "flex justify-end",
                button {
                    class: "btn-nebula text-xs disabled:opacity-40",
                    disabled: !can_submit(),
                    onclick: move |_| {
                        if !can_submit() { return; }
                        let svc_opt = if service().trim().is_empty() { None } else { Some(service()) };
                        let payload = NewTaskInput { task_name: name(), task_description: desc(), service_number: svc_opt, priority: parse_priority(&prio()), assignee_username: assignee() };
                        let cb = props.on_created.clone();
                        let mut show = props.show.to_owned();
                        spawn(async move {
                            if let Ok(created) = create_task_simple(payload).await {
                                if let Some(cb) = cb { cb.call(created); }
                            }
                            show.set(false);
                        });
                    },
                    "Create"
                }
            }
        }
    };
    rsx! { crate::components::modal::Dialog { show_modal: props.show, wrap_class: Some("w-[95%] max-w-lg".into()), children: body } }
}

fn parse_priority(s: &str) -> Priority { match s { "Express" => Priority::Express, "Rfs" => Priority::Rfs, "Fire" => Priority::Fire, "Qc" => Priority::Qc, _ => Priority::Normal } }

async fn current_user() -> anyhow::Result<User> {
    if let Ok(guard) = database::CURRENT_USER_INFO.try_lock() { if let Some(u) = guard.clone() { return Ok(u); } }
    let user: Option<User> = database::DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
    user.ok_or_else(|| anyhow::anyhow!("No current user"))
}
