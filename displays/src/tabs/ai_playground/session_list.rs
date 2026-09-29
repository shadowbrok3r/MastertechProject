//! Grouping, filtering and row text for the Ai tab's agent session list.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;

use chrono::{DateTime, Datelike, TimeDelta, TimeZone, Utc};
use database::schema::agent_thread::is_voice;
use database::schema::{AgentThread, RecordId, RecordIdExt, is_general};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    Button, Context, Id, Popup, PopupCloseBehavior, RectAlign, RichText, TextStyle, Ui,
};

use crate::ui_tools::framed_controls::{FramedSelectable, framed_menu_style};
use crate::ui_tools::{agent_chat, chat_bubble, icons, theme};

/// Group key of the sessions no technician asked for.
const UNATTRIBUTED: &str = "unattributed";
/// Longest technician name a group header shows, in characters.
const GROUP_NAME_CHARS: usize = 18;
/// Width of the filter popover.
const FILTER_MENU_W: f32 = 210.0;

/// What a session is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionKind {
    Machine,
    Records,
    Voice,
}

impl SessionKind {
    const ALL: [Self; 3] = [Self::Machine, Self::Records, Self::Voice];

    fn of(thread: &AgentThread) -> Self {
        if is_voice(&thread.connection_string) {
            Self::Voice
        } else if is_general(&thread.connection_string) {
            Self::Records
        } else {
            Self::Machine
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Machine => icons::DESKTOP,
            Self::Records => icons::RECORDS,
            Self::Voice => icons::VOICE,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Machine => "Machine",
            Self::Records => "Records",
            Self::Voice => "Voice",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Machine => "Sessions about a customer machine",
            Self::Records => "A technician's records session, with no machine in scope",
            Self::Voice => "Questions asked at the voice console",
        }
    }
}

/// A session's status as the status filter buckets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    /// Starting, running, waiting on an approval or queued.
    Working,
    Idle,
    /// Closed, failed or an unknown status.
    Ended,
}

impl SessionState {
    fn of(thread: &AgentThread) -> Self {
        match thread.status.as_str() {
            "idle" => Self::Idle,
            "queued" => Self::Working,
            _ if thread.is_busy() => Self::Working,
            _ => Self::Ended,
        }
    }
}

/// How recently a listed session was active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Age {
    All,
    Today,
    Week,
}

impl Age {
    const ALL: [Self; 3] = [Self::All, Self::Today, Self::Week];

    fn label(self) -> &'static str {
        match self {
            Self::All => "Any time",
            Self::Today => "Today",
            Self::Week => "7 days",
        }
    }

    /// Earliest last activity admitted: midnight of `now`'s day for today, a week before `now` for 7 days.
    fn since<Tz: TimeZone>(self, now: &DateTime<Tz>) -> Option<DateTime<Utc>> {
        match self {
            Self::All => None,
            Self::Today => now
                .date_naive()
                .and_hms_opt(0, 0, 0)?
                .and_local_timezone(now.timezone())
                .earliest()
                .map(|midnight| midnight.with_timezone(&Utc)),
            Self::Week => Some(now.with_timezone(&Utc) - TimeDelta::days(7)),
        }
    }
}

/// Which agent sessions the list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionFilter {
    working: bool,
    idle: bool,
    machine: bool,
    records: bool,
    voice: bool,
    age: Age,
    store: Option<String>,
}

impl Default for SessionFilter {
    fn default() -> Self {
        Self {
            working: true,
            idle: true,
            machine: true,
            records: true,
            voice: true,
            age: Age::All,
            store: None,
        }
    }
}

impl SessionFilter {
    /// Whether `thread` passes the status, kind, age and store filters, with ages read from `now`; closed and failed sessions pass the status filter.
    pub(super) fn admits<Tz: TimeZone>(&self, thread: &AgentThread, now: &DateTime<Tz>) -> bool {
        let state = match SessionState::of(thread) {
            SessionState::Working => self.working,
            SessionState::Idle => self.idle,
            SessionState::Ended => true,
        };
        let recent = self
            .age
            .since(now)
            .is_none_or(|since| last_active(thread).is_some_and(|at| at >= since));
        let store = self
            .store
            .as_deref()
            .is_none_or(|store| thread.store.as_deref().map(str::trim) == Some(store));
        state && self.shows(SessionKind::of(thread)) && recent && store
    }

    fn shows(&self, kind: SessionKind) -> bool {
        match kind {
            SessionKind::Machine => self.machine,
            SessionKind::Records => self.records,
            SessionKind::Voice => self.voice,
        }
    }

    fn kind_mut(&mut self, kind: SessionKind) -> &mut bool {
        match kind {
            SessionKind::Machine => &mut self.machine,
            SessionKind::Records => &mut self.records,
            SessionKind::Voice => &mut self.voice,
        }
    }

    /// True when a filter hides sessions the default list shows.
    pub(super) fn narrows(&self) -> bool {
        let every_open = self.working && self.idle;
        let every_kind = self.machine && self.records && self.voice;
        !every_open || !every_kind || self.age != Age::All || self.store.is_some()
    }

    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The set filters in a few words, such as `Working · Voice · Today`.
    pub(super) fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !(self.working && self.idle) {
            let states = [(self.working, "Working"), (self.idle, "Idle")];
            push_chosen(&mut parts, &states, "no status");
        }
        if !(self.machine && self.records && self.voice) {
            let kinds = SessionKind::ALL.map(|kind| (self.shows(kind), kind.label()));
            push_chosen(&mut parts, &kinds, "no kind");
        }
        if self.age != Age::All {
            parts.push(self.age.label());
        }
        if let Some(store) = &self.store {
            parts.push(store);
        }
        parts.join(" \u{00b7} ")
    }
}

/// Pushes the words of the chosen options, or `none` when no option is chosen.
fn push_chosen<'a>(parts: &mut Vec<&'a str>, options: &[(bool, &'a str)], none: &'a str) {
    let before = parts.len();
    parts.extend(options.iter().filter(|(on, _)| *on).map(|(_, word)| *word));
    if parts.len() == before {
        parts.push(none);
    }
}

/// The session's last update, else its start.
fn last_active(thread: &AgentThread) -> Option<DateTime<Utc>> {
    thread.updated_at.or(thread.created_at).map(DateTime::from)
}

/// Active users' names and emails and the signed-in user's email, as last read.
#[derive(Debug, Default)]
pub(super) struct Roster {
    /// Name by lowercased email.
    names: HashMap<String, String>,
    /// Lowercased email by user record.
    emails: HashMap<RecordId, String>,
    /// The signed-in user's lowercased email.
    me: Option<String>,
}

impl Roster {
    /// Reads the signed-in user and the active user list, leaving out either while its lock is busy.
    pub(super) fn load() -> Self {
        let me = database::CURRENT_USER_INFO
            .try_lock()
            .ok()
            .and_then(|user| user.as_ref().map(|u| email_key(u.get_email())));
        match database::STORE_USERS.try_lock() {
            Ok(users) => Self::from_users(
                users
                    .iter()
                    .map(|u| (u.get_id(), u.get_email(), u.get_name())),
                me,
            ),
            Err(_) => Self {
                me,
                ..Self::default()
            },
        }
    }

    fn from_users<'a>(
        users: impl IntoIterator<Item = (RecordId, &'a str, &'a str)>,
        me: Option<String>,
    ) -> Self {
        let mut roster = Self {
            me,
            ..Self::default()
        };
        for (id, email, name) in users {
            let email = email_key(email);
            if email.is_empty() {
                continue;
            }
            if !name.trim().is_empty() {
                roster.names.insert(email.clone(), name.trim().to_string());
            }
            roster.emails.insert(id, email);
        }
        roster
    }

    /// Group key of the session's technician: the requester's email, else the assignee's; `None` when unattributed.
    fn owner(&self, thread: &AgentThread) -> Option<String> {
        let requester = thread.requested_by.as_deref().map(email_key);
        requester.filter(|email| !email.is_empty()).or_else(|| {
            let id = thread.assignee.as_ref()?;
            Some(
                self.emails
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| id.key_string()),
            )
        })
    }

    /// The user's name for `key`, else the email's local part.
    fn name(&self, key: &str) -> String {
        match self.names.get(key) {
            Some(name) => name.clone(),
            None => key.split('@').next().unwrap_or(key).to_string(),
        }
    }

    /// The session's technician by name, or `Unattributed`.
    pub(super) fn tech_of(&self, thread: &AgentThread) -> String {
        match self.owner(thread) {
            Some(key) => self.name(&key),
            None => "Unattributed".to_string(),
        }
    }
}

fn email_key(email: &str) -> String {
    email.trim().to_lowercase()
}

/// One technician's sessions, or the unattributed ones.
#[derive(Debug)]
pub(super) struct SessionGroup<'a> {
    /// The technician's lowercased email; `None` for the unattributed group.
    key: Option<String>,
    name: String,
    mine: bool,
    pub(super) rows: Vec<&'a AgentThread>,
}

impl SessionGroup<'_> {
    /// Key of the group's collapsed state and header.
    pub(super) fn id(&self) -> &str {
        self.key.as_deref().unwrap_or(UNATTRIBUTED)
    }

    /// Hover text for the unattributed group's header.
    pub(super) fn hint(&self) -> Option<&'static str> {
        self.key
            .is_none()
            .then_some("Sessions no technician asked for, such as guest voice questions")
    }

    fn working(&self) -> usize {
        self.rows
            .iter()
            .filter(|thread| SessionState::of(thread) == SessionState::Working)
            .count()
    }

    fn rank(&self) -> u8 {
        match (&self.key, self.mine) {
            (_, true) => 0,
            (Some(_), false) => 1,
            (None, false) => 2,
        }
    }
}

/// `rows` by technician: the viewer's first, then by name, the unattributed last; rows working first, then newest.
pub(super) fn group_by_tech<'a>(
    rows: &[&'a AgentThread],
    roster: &Roster,
) -> Vec<SessionGroup<'a>> {
    let mut groups: Vec<SessionGroup<'a>> = Vec::new();
    for &thread in rows {
        let key = roster.owner(thread);
        match groups.iter_mut().find(|group| group.key == key) {
            Some(group) => group.rows.push(thread),
            None => groups.push(SessionGroup {
                name: key
                    .as_deref()
                    .map_or_else(|| "Unattributed".to_string(), |key| roster.name(key)),
                mine: key.is_some() && key == roster.me,
                key,
                rows: vec![thread],
            }),
        }
    }
    groups.sort_by_cached_key(|group| (group.rank(), group.name.to_lowercase()));
    for group in &mut groups {
        working_first(&mut group.rows);
    }
    groups
}

/// Sorts working sessions first, then by newest update and newest start.
pub(super) fn working_first(rows: &mut [&AgentThread]) {
    rows.sort_by_key(|thread| {
        let working = SessionState::of(thread) == SessionState::Working;
        std::cmp::Reverse((working, thread.updated_at, thread.created_at))
    });
}

/// Whether the lowercased `needle` is in the session's title, technician, machine, service number, store or status.
pub(super) fn mentions(thread: &AgentThread, roster: &Roster, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let hit = |field: &str| field.to_lowercase().contains(needle);
    let fields = [
        thread.requested_by.as_deref(),
        Some(thread.connection_string.as_str()),
        thread.hostname.as_deref(),
        thread.store.as_deref(),
        Some(thread.status.as_str()),
    ];
    hit(&thread.label())
        || hit(&roster.tech_of(thread))
        || fields.into_iter().flatten().any(hit)
        || thread
            .service_number
            .as_deref()
            .is_some_and(|sn| hit(&format!("#{sn}")))
        || hit(&agent_chat::status_words(thread))
}

/// `11:54` when `at` falls on `now`'s day, else `Sep 28`, with the year when it differs from `now`'s.
fn start_label<Tz: TimeZone>(at: DateTime<Utc>, now: &DateTime<Tz>) -> String
where
    Tz::Offset: Display,
{
    let at = at.with_timezone(&now.timezone());
    let format = if at.date_naive() == now.date_naive() {
        "%H:%M"
    } else if at.year() == now.year() {
        "%b %d"
    } else {
        "%b %d %Y"
    };
    at.format(format).to_string()
}

/// A row's second line: the kind icon, then the technician when given, the start time and the status.
pub(super) fn detail_line<Tz: TimeZone>(
    thread: &AgentThread,
    tech: Option<&str>,
    now: &DateTime<Tz>,
) -> String
where
    Tz::Offset: Display,
{
    let started = thread
        .created_at
        .map(|at| start_label(DateTime::from(at), now));
    let parts: Vec<String> = tech
        .map(str::to_string)
        .into_iter()
        .chain(started)
        .chain([agent_chat::status_words(thread)])
        .collect();
    format!(
        "{} {}",
        SessionKind::of(thread).icon(),
        parts.join(" \u{00b7} ")
    )
}

/// A row's hover text: title, technician, connection, then start, last activity and store.
pub(super) fn hover_text<Tz: TimeZone>(
    thread: &AgentThread,
    tech: &str,
    now: &DateTime<Tz>,
) -> String
where
    Tz::Offset: Display,
{
    let clock = |at| {
        chat_bubble::clock_label(
            &DateTime::<Utc>::from(at).with_timezone(&now.timezone()),
            now,
        )
    };
    let who = match thread.requested_by.as_deref() {
        Some(email) if !email.eq_ignore_ascii_case(tech) => format!("{tech} <{email}>"),
        _ => tech.to_string(),
    };
    let mut when = Vec::new();
    if let Some(at) = thread.created_at {
        when.push(format!("started {}", clock(at)));
    }
    if let Some(at) = thread.updated_at {
        when.push(format!("active {}", clock(at)));
    }
    if let Some(store) = thread.store.as_deref().filter(|s| !s.trim().is_empty()) {
        when.push(store.to_string());
    }
    let mut lines = vec![thread.label(), who, thread.connection_string.clone()];
    if !when.is_empty() {
        lines.push(when.join(" \u{00b7} "));
    }
    lines.join("\n")
}

/// Distinct stores the sessions name, sorted.
pub(super) fn stores(index: &[AgentThread]) -> Vec<String> {
    index
        .iter()
        .filter_map(|thread| thread.store.as_deref().map(str::trim))
        .filter(|store| !store.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// A group's header: the technician, a `(you)` mark, the session count and how many are working.
pub(super) fn group_header(ui: &Ui, group: &SessionGroup<'_>) -> LayoutJob {
    let font = TextStyle::Body.resolve(ui.style());
    let weak = TextFormat::simple(font.clone(), theme::weak_text(ui));
    let mut job = LayoutJob::default();
    let name = chat_bubble::clip(&group.name, GROUP_NAME_CHARS);
    job.append(
        &name,
        0.0,
        TextFormat::simple(font.clone(), ui.visuals().text_color()),
    );
    if group.mine {
        job.append(" (you)", 0.0, weak.clone());
    }
    job.append(&format!("  {}", group.rows.len()), 0.0, weak);
    let working = group.working();
    if working > 0 {
        job.append(
            &format!("  \u{00b7} {working} working"),
            0.0,
            TextFormat::simple(font, theme::info(ui)),
        );
    }
    job
}

fn filter_popup_id() -> Id {
    Id::new("enhanced_ai_session_filter")
}

/// Whether the filter popover is open.
pub(super) fn filter_open(ctx: &Context) -> bool {
    Popup::is_id_open(ctx, filter_popup_id())
}

/// The filter button, highlighted while a filter is set, and its popover with the grouping toggle.
pub(super) fn filter_button(
    ui: &mut Ui,
    filter: &mut SessionFilter,
    by_tech: &mut bool,
    stores: &[String],
) {
    let active = !filter.is_default();
    let tip = if active {
        format!("Filtered: {}", filter.summary())
    } else {
        "Filter sessions".to_string()
    };
    let button = ui
        .add(Button::new(icons::FILTER).selected(active))
        .on_hover_text(tip);
    Popup::menu(&button)
        .id(filter_popup_id())
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END)
        .style(framed_menu_style)
        .width(FILTER_MENU_W)
        .show(|ui| filter_menu(ui, filter, by_tech, stores));
}

fn filter_menu(ui: &mut Ui, filter: &mut SessionFilter, by_tech: &mut bool, stores: &[String]) {
    ui.set_max_width(FILTER_MENU_W);
    caption(ui, "Status");
    ui.horizontal_wrapped(|ui| {
        chip(
            ui,
            &mut filter.working,
            "Working",
            "Starting, running, waiting on an approval or queued",
        );
        chip(
            ui,
            &mut filter.idle,
            "Idle",
            "Open and waiting for a message",
        );
    });
    caption(ui, "Kind");
    ui.horizontal_wrapped(|ui| {
        for kind in SessionKind::ALL {
            let label = format!("{} {}", kind.icon(), kind.label());
            chip(ui, filter.kind_mut(kind), &label, kind.hint());
        }
    });
    caption(ui, "Last active");
    ui.horizontal_wrapped(|ui| {
        for age in Age::ALL {
            ui.framed_selectable_value(&mut filter.age, age, age.label());
        }
    });
    if !stores.is_empty() {
        caption(ui, "Store");
        ui.horizontal_wrapped(|ui| {
            ui.framed_selectable_value(&mut filter.store, None, "All");
            for store in stores {
                ui.framed_selectable_value(&mut filter.store, Some(store.clone()), store.as_str());
            }
        });
    }
    ui.separator();
    ui.checkbox(by_tech, "Group by technician");
    let reset = Button::new(format!("{} Reset filters", icons::UNDO));
    if ui.add_enabled(!filter.is_default(), reset).clicked() {
        *filter = SessionFilter::default();
    }
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

/// A chip that flips one filter flag.
fn chip(ui: &mut Ui, on: &mut bool, text: &str, hint: &str) {
    if ui
        .framed_selectable_label(*on, text)
        .on_hover_text(hint)
        .clicked()
    {
        *on = !*on;
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use database::schema::Datetime;

    const LOGAN: &str = "logan.lees@pclaptops.com";
    const JACOB: &str = "jacob.hardy@pclaptops.com";

    pub(in crate::tabs::ai_playground) fn thread(
        key: &str,
        status: &str,
        connection_string: &str,
        requested_by: Option<&str>,
    ) -> AgentThread {
        AgentThread {
            id: RecordId::new("agent_thread", key),
            status: status.into(),
            connection_string: connection_string.into(),
            hostname: None,
            service_number: None,
            store: None,
            requested_by: requested_by.map(str::to_string),
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
            last_seq: None,
            activity: None,
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        }
    }

    fn at(secs: i64) -> Option<Datetime> {
        Datetime::from_timestamp(secs, 0)
    }

    /// 2026-09-29 11:54:00 UTC.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 29, 11, 54, 0)
            .single()
            .expect("valid time")
    }

    fn roster() -> Roster {
        Roster::from_users(
            [
                (
                    RecordId::new("user", "logan"),
                    "Logan.Lees@pclaptops.com",
                    "Logan Lees",
                ),
                (RecordId::new("user", "jacob"), JACOB, " Jacob Hardy "),
                (
                    RecordId::new("user", "nameless"),
                    "sam.smith@pclaptops.com",
                    "",
                ),
            ],
            Some(LOGAN.into()),
        )
    }

    #[test]
    fn statuses_and_connections_fall_into_their_buckets() {
        for (status, state) in [
            ("queued", SessionState::Working),
            ("starting", SessionState::Working),
            ("running", SessionState::Working),
            ("waiting_approval", SessionState::Working),
            ("idle", SessionState::Idle),
            ("closed", SessionState::Ended),
            ("failed", SessionState::Ended),
            ("something new", SessionState::Ended),
        ] {
            assert_eq!(
                SessionState::of(&thread("t", status, "PC:1", None)),
                state,
                "{status}"
            );
        }
        for (cs, kind) in [
            ("JeffsComputer:663a3fd40", SessionKind::Machine),
            ("general:jacob.hardy@pclaptops.com", SessionKind::Records),
            ("general:voice:guest:1790000000000", SessionKind::Voice),
        ] {
            assert_eq!(
                SessionKind::of(&thread("t", "idle", cs, None)),
                kind,
                "{cs}"
            );
        }
    }

    #[test]
    fn the_default_filter_admits_every_status() {
        let filter = SessionFilter::default();
        assert!(filter.is_default() && !filter.narrows());
        for cs in ["PC:1", "general:t@x.com", "general:voice:guest:1"] {
            assert!(
                filter.admits(&thread("a", "running", cs, None), &now()),
                "{cs}"
            );
            assert!(
                filter.admits(&thread("b", "idle", cs, None), &now()),
                "{cs}"
            );
            assert!(
                filter.admits(&thread("c", "closed", cs, None), &now()),
                "{cs}"
            );
        }
    }

    #[test]
    fn filters_narrow_by_status_kind_and_store() {
        let working = SessionFilter {
            idle: false,
            ..SessionFilter::default()
        };
        assert!(working.admits(&thread("a", "queued", "PC:1", None), &now()));
        assert!(!working.admits(&thread("b", "idle", "PC:1", None), &now()));
        assert!(working.narrows());

        let no_voice = SessionFilter {
            voice: false,
            ..SessionFilter::default()
        };
        assert!(!no_voice.admits(&thread("a", "idle", "general:voice:guest:1", None), &now()));
        assert!(no_voice.admits(&thread("b", "idle", "general:t@x.com", None), &now()));

        let orem = SessionFilter {
            store: Some("Orem".into()),
            ..SessionFilter::default()
        };
        let mut at_orem = thread("a", "idle", "PC:1", None);
        at_orem.store = Some(" Orem ".into());
        assert!(orem.admits(&at_orem, &now()));
        assert!(!orem.admits(&thread("b", "idle", "PC:1", None), &now()));
    }

    #[test]
    fn the_age_filter_reads_the_last_update_else_the_start() {
        let today = SessionFilter {
            age: Age::Today,
            ..SessionFilter::default()
        };
        let week = SessionFilter {
            age: Age::Week,
            ..SessionFilter::default()
        };
        let midnight = Utc
            .with_ymd_and_hms(2026, 9, 29, 0, 0, 0)
            .single()
            .expect("valid time")
            .timestamp();
        let mut row = thread("a", "idle", "PC:1", None);
        row.created_at = at(midnight - 3 * 86_400);
        row.updated_at = at(midnight + 60);
        assert!(today.admits(&row, &now()) && week.admits(&row, &now()));
        row.updated_at = at(midnight - 60);
        assert!(!today.admits(&row, &now()) && week.admits(&row, &now()));
        row.updated_at = None;
        assert!(!today.admits(&row, &now()) && week.admits(&row, &now()));
        row.created_at = at(midnight - 8 * 86_400);
        assert!(!week.admits(&row, &now()));
        row.created_at = None;
        assert!(!week.admits(&row, &now()) && SessionFilter::default().admits(&row, &now()));
    }

    #[test]
    fn the_summary_names_the_set_filters() {
        assert_eq!(SessionFilter::default().summary(), "");
        let set = SessionFilter {
            idle: false,
            machine: false,
            records: false,
            age: Age::Today,
            store: Some("Orem".into()),
            ..SessionFilter::default()
        };
        assert_eq!(
            set.summary(),
            "Working \u{00b7} Voice \u{00b7} Today \u{00b7} Orem"
        );
        let nothing = SessionFilter {
            working: false,
            idle: false,
            ..SessionFilter::default()
        };
        assert_eq!(nothing.summary(), "no status");
    }

    #[test]
    fn groups_put_the_viewer_first_then_names_then_the_unattributed() {
        let mut by_assignee = thread("e", "idle", "PC:5", None);
        by_assignee.assignee = Some(RecordId::new("user", "jacob"));
        let mut unknown_assignee = thread("f", "idle", "PC:6", None);
        unknown_assignee.assignee = Some(RecordId::new("user", "gone"));
        let rows = [
            thread("a", "idle", "PC:1", Some("tyler.naylor@pclaptops.com")),
            thread(
                "b",
                "idle",
                "general:jacob.hardy@pclaptops.com",
                Some(JACOB),
            ),
            thread("c", "idle", "general:voice:guest:1", None),
            thread(
                "d",
                "idle",
                "general:logan.lees@pclaptops.com",
                Some("Logan.Lees@PCLaptops.com"),
            ),
            by_assignee,
            unknown_assignee,
            thread("g", "idle", "PC:7", Some("sam.smith@pclaptops.com")),
        ];
        let refs: Vec<&AgentThread> = rows.iter().collect();
        let groups = group_by_tech(&refs, &roster());
        let seen: Vec<(&str, &str, bool, usize)> = groups
            .iter()
            .map(|g| (g.id(), g.name.as_str(), g.mine, g.rows.len()))
            .collect();
        assert_eq!(
            seen,
            [
                (LOGAN, "Logan Lees", true, 1),
                ("gone", "gone", false, 1),
                (JACOB, "Jacob Hardy", false, 2),
                ("sam.smith@pclaptops.com", "sam.smith", false, 1),
                ("tyler.naylor@pclaptops.com", "tyler.naylor", false, 1),
                (UNATTRIBUTED, "Unattributed", false, 1),
            ]
        );
    }

    #[test]
    fn rows_list_working_sessions_first_then_the_newest_update() {
        let mut old_idle = thread("old", "idle", "PC:1", None);
        old_idle.updated_at = at(200);
        let mut new_idle = thread("new", "idle", "PC:2", None);
        new_idle.updated_at = at(300);
        let mut queued = thread("queued", "queued", "PC:3", None);
        queued.updated_at = at(100);
        let mut running = thread("running", "running", "PC:4", None);
        running.updated_at = at(150);
        let never = thread("never", "idle", "PC:5", None);
        let rows = [old_idle, never, queued, new_idle, running];
        let mut refs: Vec<&AgentThread> = rows.iter().collect();
        working_first(&mut refs);
        let order: Vec<String> = refs.iter().map(|t| t.id.key_string()).collect();
        assert_eq!(order, ["running", "queued", "new", "old", "never"]);
    }

    #[test]
    fn the_search_reads_title_tech_machine_service_number_and_status() {
        let mut row = thread("a", "running", "JeffsComputer:663a3fd40", Some(JACOB));
        row.title = Some("Why is it slow".into());
        row.service_number = Some("2155485".into());
        row.hostname = Some("JeffsComputer".into());
        row.activity = Some("tool:get_client_info".into());
        let roster = roster();
        for needle in [
            "",
            "slow",
            "jacob hardy",
            "jacob.hardy@",
            "jeffscomputer",
            "#2155485",
            "2155485",
            "running get_client_info",
        ] {
            assert!(mentions(&row, &roster, needle), "{needle}");
        }
        assert!(!mentions(&row, &roster, "tyler"));
    }

    #[test]
    fn start_labels_show_the_clock_today_and_the_date_before() {
        let when = |y, m, d, h, min| {
            Utc.with_ymd_and_hms(y, m, d, h, min, 0)
                .single()
                .expect("valid time")
        };
        assert_eq!(start_label(when(2026, 9, 29, 8, 5), &now()), "08:05");
        assert_eq!(start_label(when(2026, 9, 28, 23, 59), &now()), "Sep 28");
        assert_eq!(start_label(when(2025, 9, 28, 12, 0), &now()), "Sep 28 2025");
    }

    #[test]
    fn detail_lines_name_the_tech_only_when_given() {
        let mut row = thread("a", "idle", "general:voice:guest:1", None);
        row.created_at = Some(Datetime::from(now()));
        assert_eq!(
            detail_line(&row, None, &now()),
            format!("{} 11:54 \u{00b7} Idle", icons::VOICE)
        );
        assert_eq!(
            detail_line(&row, Some("jacob.hardy"), &now()),
            format!("{} jacob.hardy \u{00b7} 11:54 \u{00b7} Idle", icons::VOICE)
        );
        row.created_at = None;
        row.connection_string = "PC:1".into();
        row.status = "closed".into();
        assert_eq!(
            detail_line(&row, None, &now()),
            format!("{} Closed", icons::DESKTOP)
        );
    }

    #[test]
    fn hover_text_names_the_tech_and_the_connection() {
        let mut row = thread("a", "idle", "JeffsComputer:663a3fd40", Some(JACOB));
        row.service_number = Some("2155485".into());
        row.hostname = Some("JeffsComputer".into());
        row.store = Some("Orem".into());
        row.created_at = Some(Datetime::from(now()));
        let text = hover_text(&row, "Jacob Hardy", &now());
        assert_eq!(
            text,
            format!(
                "#2155485 JeffsComputer\nJacob Hardy <{JACOB}>\nJeffsComputer:663a3fd40\nstarted 11:54 \u{00b7} Orem"
            )
        );
    }

    #[test]
    fn stores_are_distinct_and_sorted() {
        let with_store = |key: &str, store: Option<&str>| {
            let mut row = thread(key, "idle", "PC:1", None);
            row.store = store.map(str::to_string);
            row
        };
        let rows = [
            with_store("a", Some("Provo")),
            with_store("b", Some(" Orem")),
            with_store("c", None),
            with_store("d", Some("Orem ")),
            with_store("e", Some("  ")),
        ];
        assert_eq!(stores(&rows), ["Orem", "Provo"]);
    }
}
