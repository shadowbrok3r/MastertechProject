use crate::mcp_tool_log::{self, McpToolCallLog, McpToolCallStatus};
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};
use eframe::egui::{Align, Button, Color32, Layout, RichText, ScrollArea, TextEdit, Ui, Widget};
use egui_json_tree::{
    render::{DefaultRender, RenderContext},
    DefaultExpand, JsonTree, JsonTreeStyle, JsonTreeVisuals,
};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// State for one RecordID tab in the breadcrumb. The id is the
/// canonical `table:key` string the user clicked; `short_label` is a
/// truncated display form so a 64-character UUID doesn't blow out the
/// breadcrumb row.
struct RecordIdTab {
    id: String,
    short_label: String,
}

#[derive(Clone, Debug)]
enum RecordView {
    Loading,
    Loaded(JsonValue),
    Error(String),
}

type RecordCache = Arc<Mutex<HashMap<String, RecordView>>>;

pub struct McpToolLogViewer {
    expanded: HashSet<String>,
    filter: String,
    show_completed: bool,
    auto_scroll: bool,
    /// Breadcrumb of RecordIDs the operator has clicked into. Empty
    /// means we're on the "Tool Calls" log view.
    breadcrumb: Vec<RecordIdTab>,
    /// Index of the currently-active tab in `breadcrumb`. `None` = the
    /// log view (the original landing screen).
    active_tab: Option<usize>,
    /// Shared cache of fetched record bodies. Populated asynchronously
    /// by background DB queries so the egui thread never blocks.
    record_cache: RecordCache,
}

impl Default for McpToolLogViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpToolLogViewer {
    pub fn new() -> Self {
        Self {
            expanded: HashSet::new(),
            filter: String::new(),
            show_completed: true,
            auto_scroll: true,
            breadcrumb: Vec::new(),
            active_tab: None,
            record_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn display(&mut self, ui: &mut Ui, connection_string: &str) {
        let entries = mcp_tool_log::get_for_client(connection_string);
        let pending = entries
            .iter()
            .filter(|e| matches!(e.status, McpToolCallStatus::Pending))
            .count();

        // ── Row 1: controls (Show completed / Auto-scroll / Filter / Clear) ──
        ui.horizontal(|ui| {
            ui.label(RichText::new("MCP Tool Calls").strong());
            ui.separator();
            ui.label(format!("{} total", entries.len()));
            if pending > 0 {
                ui.colored_label(
                    Color32::from_rgb(255, 200, 80),
                    format!("{} {pending} in flight", icons::STATUS_WAIT),
                );
            }
            ui.separator();
            ui.checkbox(&mut self.show_completed, "Show completed");
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");
            ui.separator();
            ui.label("Filter:");
            ui.add(TextEdit::singleline(&mut self.filter).desired_width(160.0));
            if ui.button("Clear completed").clicked() {
                mcp_tool_log::clear(connection_string);
            }
        });

        // Build the filtered list now so the Copy button reflects what's
        // visible AND we don't redo the work in display_log_entries.
        let filter = self.filter.to_ascii_lowercase();
        let filter_active = !filter.is_empty();
        let filtered: Vec<McpToolCallLog> = entries
            .into_iter()
            .filter(|e| {
                if !self.show_completed && !matches!(e.status, McpToolCallStatus::Pending) {
                    return false;
                }
                if filter_active {
                    let hay = format!("{} {} {}", e.plugin_id, e.tool_name, e.args_json)
                        .to_ascii_lowercase();
                    if !hay.contains(&filter) {
                        return false;
                    }
                }
                true
            })
            .collect();

        // ── Row 2: breadcrumb (left) + Copy button (right-aligned) ──
        let mut breadcrumb_click: Option<Option<usize>> = None;
        let mut breadcrumb_clear = false;
        ui.horizontal(|ui| {
            // Right-aligned Copy button first so it doesn't fight the
            // breadcrumb for width — egui's right_to_left layout fills
            // from the right edge.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = format!("Copy {} entry/entries", filtered.len());
                let copy_enabled = !filtered.is_empty();
                if ui
                    .add_enabled(copy_enabled, eframe::egui::Button::new(label))
                    .on_hover_text(
                        "Copy the currently-visible MCP tool calls to the clipboard as plain text.",
                    )
                    .clicked()
                {
                    ui.ctx().copy_text(format_entries_for_clipboard(&filtered));
                }

                // Now flip back to LTR for the breadcrumb on the left side.
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    // Always-visible "Tool Calls" home pseudo-tab so the
                    // operator has a way back to the log without an
                    // X-button hunt.
                    let home_active = self.active_tab.is_none();
                    let home_label = format!("{} Tool Calls", icons::CLIPBOARD);
                    if ui
                        .selectable_label(home_active, home_label)
                        .on_hover_text("Back to the MCP tool calls log")
                        .clicked()
                    {
                        breadcrumb_click = Some(None);
                    }
                    for (i, tab) in self.breadcrumb.iter().enumerate() {
                        ui.label(icons::CARET_RIGHT);
                        let is_active = self.active_tab == Some(i);
                        if ui
                            .selectable_label(is_active, &tab.short_label)
                            .on_hover_text(&tab.id)
                            .clicked()
                        {
                            breadcrumb_click = Some(Some(i));
                        }
                    }
                    if !self.breadcrumb.is_empty() {
                        ui.add_space(6.0);
                        if ui
                            .small_button(icons::CLOSE)
                            .on_hover_text("Clear breadcrumb trail")
                            .clicked()
                        {
                            breadcrumb_clear = true;
                        }
                    }
                });
            });
        });
        if breadcrumb_clear {
            self.breadcrumb.clear();
            self.active_tab = None;
        }
        if let Some(target) = breadcrumb_click {
            self.active_tab = target;
        }
        ui.separator();

        // ── Main area: either the log entries OR a record view ──
        match self.active_tab {
            None => self.display_log_entries(ui, &filtered),
            Some(idx) => {
                // Clone the id out so we don't borrow `self` immutably
                // while we want it mutable for the record view.
                let id = self.breadcrumb.get(idx).map(|t| t.id.clone());
                if let Some(id) = id {
                    self.display_record_view(ui, &id);
                } else {
                    self.active_tab = None;
                }
            }
        }
    }

    fn display_log_entries(&mut self, ui: &mut Ui, filtered: &[McpToolCallLog]) {
        if filtered.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No MCP tool calls to show.")
                        .color(Color32::GRAY)
                        .small(),
                );
                ui.label(
                    RichText::new(
                        "Calls proxied through this client's Web Console session will appear here.",
                    )
                    .color(Color32::from_rgb(120, 120, 140))
                    .small(),
                );
            });
            return;
        }

        // RecordIDs clicked anywhere in this frame's render. Drained
        // after the scroll area runs so `open_record` can mutate the
        // breadcrumb without fighting the immutable borrow that the
        // row iteration needs.
        let mut clicks_to_open: Vec<String> = Vec::new();

        let mut scroll = ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("mcp-tool-log-scroll");
        if self.auto_scroll {
            scroll = scroll.stick_to_bottom(true);
        }
        scroll.show(ui, |ui| {
            for entry in filtered {
                self.row(ui, entry, &mut clicks_to_open);
            }
        });

        for id in clicks_to_open {
            self.open_record(id);
        }
    }

    fn display_record_view(&mut self, ui: &mut Ui, id: &str) {
        // Ensure a fetch is in flight (or already cached).
        self.ensure_record_fetched(id);

        let view = {
            let Ok(g) = self.record_cache.lock() else {
                ui.colored_label(Color32::LIGHT_RED, "record cache poisoned");
                return;
            };
            g.get(id).cloned()
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new("Record:").color(Color32::from_rgb(180, 200, 230)));
            ui.label(RichText::new(id).monospace());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .small_button(format!("{} Refetch", icons::REFRESH))
                    .clicked()
                {
                    if let Ok(mut g) = self.record_cache.lock() {
                        g.remove(id);
                    }
                    self.ensure_record_fetched(id);
                }
            });
        });
        ui.separator();

        let mut clicks_to_open: Vec<String> = Vec::new();
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt(format!("mcp-record-scroll-{id}"))
            .show(ui, |ui| match view {
                Some(RecordView::Loading) | None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.colored_label(
                            Color32::from_rgb(255, 200, 80),
                            "Loading record from SurrealDB…",
                        );
                    });
                }
                Some(RecordView::Loaded(value)) => {
                    json_tree(ui, &format!("record-{id}"), &value, &mut clicks_to_open);
                }
                Some(RecordView::Error(e)) => {
                    ui.colored_label(Color32::LIGHT_RED, format!("Fetch failed: {e}"));
                }
            });
        for next_id in clicks_to_open {
            self.open_record(next_id);
        }
    }

    fn row(&mut self, ui: &mut Ui, entry: &McpToolCallLog, clicks: &mut Vec<String>) {
        let is_expanded = self.expanded.contains(&entry.request_id);
        let (icon, color) = status_glyph(&entry.status);
        let record_ids = extract_record_ids_from_entry(entry);

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.colored_label(color, icon);
                let header = format!(
                    "{}::{}",
                    short_id(&entry.plugin_id),
                    entry.tool_name
                );
                
                let toggle = Button::selectable(
                    is_expanded, 
                    RichText::new(header).monospace())
                    .frame_when_inactive(true)
                    .ui(ui);

                if toggle.clicked() {
                    if is_expanded {
                        self.expanded.remove(&entry.request_id);
                    } else {
                        self.expanded.insert(entry.request_id.clone());
                    }
                }

                // Inline RecordID buttons — every distinct id we found
                // in this entry's args + result shows up as a clickable
                // chip next to the tool name.
                for rid in &record_ids {
                    if ui
                        .small_button(short_id(rid))
                        .on_hover_text(format!("Open record: {rid}"))
                        .clicked()
                    {
                        clicks.push(rid.clone());
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let elapsed = format_elapsed(entry.elapsed_ms());
                    ui.label(
                        RichText::new(elapsed)
                            .color(Color32::from_rgb(160, 160, 170))
                            .small(),
                    );
                    ui.label(
                        RichText::new(format!("req={}", short_id(&entry.request_id)))
                            .color(Color32::from_rgb(120, 120, 140))
                            .small()
                            .monospace(),
                    );
                });
            });

            if !is_expanded {
                let preview = arg_preview(&entry.args_json);
                if !preview.is_empty() {
                    ui.label(
                        RichText::new(preview)
                            .color(Color32::from_rgb(180, 180, 200))
                            .small()
                            .monospace(),
                    );
                }
                return;
            }

            // Expanded: full args + result via egui_json_tree, rendered
            // inline (no inner scroll) so the parent scroll area handles
            // overflow.
            ui.separator();
            ui.label(
                RichText::new("Arguments")
                    .color(Color32::from_rgb(180, 200, 230))
                    .small(),
            );
            if let Some(args) = parse_json(&entry.args_json) {
                json_tree(ui, &format!("args-{}", entry.request_id), &args, clicks);
            } else {
                ui.label(RichText::new(&entry.args_json).monospace().small());
            }

            ui.add_space(4.0);
            ui.label(
                RichText::new("Result")
                    .color(Color32::from_rgb(180, 200, 230))
                    .small(),
            );
            match (&entry.status, entry.result_json.as_deref()) {
                (McpToolCallStatus::Pending, _) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.colored_label(
                            Color32::from_rgb(255, 200, 80),
                            "Awaiting response from remote client…",
                        );
                    });
                }
                (_, Some(body)) => {
                    if let Some(parsed) = parse_json(body) {
                        json_tree(ui, &format!("res-{}", entry.request_id), &parsed, clicks);
                    } else {
                        ui.label(RichText::new(body).monospace().small());
                    }
                }
                (_, None) => {
                    ui.colored_label(Color32::GRAY, "(no result body)");
                }
            }
        });
    }

    pub fn open_record(&mut self, id: String) {
        // If already in breadcrumb, jump to that tab rather than
        // appending a duplicate — keeps the trail tidy when the
        // operator clicks the same id twice.
        if let Some(existing) = self.breadcrumb.iter().position(|t| t.id == id) {
            self.active_tab = Some(existing);
            return;
        }
        // Truncate forward history: clicking a new id from a
        // previously-active tab discards anything past that point, same
        // as a web browser's back/new-link behaviour.
        if let Some(idx) = self.active_tab {
            self.breadcrumb.truncate(idx + 1);
        }
        self.breadcrumb.push(RecordIdTab {
            short_label: short_id(&id),
            id: id.clone(),
        });
        self.active_tab = Some(self.breadcrumb.len() - 1);
        self.ensure_record_fetched(&id);
    }

    fn ensure_record_fetched(&self, id: &str) {
        let already = self
            .record_cache
            .lock()
            .ok()
            .map(|g| g.contains_key(id))
            .unwrap_or(true);
        if already {
            return;
        }
        if let Ok(mut g) = self.record_cache.lock() {
            g.insert(id.to_string(), RecordView::Loading);
        }
        let cache = self.record_cache.clone();
        let id_owned = id.to_string();
        PlatformSpawner::spawn(async move {
            let outcome = fetch_record(&id_owned).await;
            if let Ok(mut g) = cache.lock() {
                g.insert(
                    id_owned,
                    match outcome {
                        Ok(v) => RecordView::Loaded(v),
                        Err(e) => RecordView::Error(e),
                    },
                );
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_record(id_str: &str) -> Result<JsonValue, String> {
    let (tbl, key) = parse_record_id_components(id_str)
        .ok_or_else(|| format!("not a parseable RecordID: {id_str:?}"))?;
    let rid = database::schema::RecordId::new(tbl, key);
    let result: Option<JsonValue> = database::DATABASE
        .query("SELECT * FROM $rid")
        .bind(("rid", rid))
        .await
        .map_err(|e| e.to_string())?
        .take(0)
        .map_err(|e| e.to_string())?;
    result.ok_or_else(|| "record not found".to_string())
}

#[cfg(target_arch = "wasm32")]
async fn fetch_record(_id_str: &str) -> Result<JsonValue, String> {
    Err("record fetch unavailable on wasm".to_string())
}

fn json_tree(ui: &mut Ui, id_salt: &str, value: &JsonValue, clicks: &mut Vec<String>) {
    let style = JsonTreeStyle::new().visuals(themed_json_visuals(ui));
    let clicks_cell: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let clicks_inner = clicks_cell.clone();
        JsonTree::new(id_salt, value)
            .style(style)
            .default_expand(DefaultExpand::ToLevel(1))
            .on_render(move |ui, ctx| {
                if let RenderContext::BaseValue(ref base) = ctx {
                    if base.value_type == egui_json_tree::value::BaseValueType::String {
                        let raw = base.display_value.to_string();
                        if is_record_id_string(&raw) {
                            let canonical = canonical_record_id(&raw);
                            ui.horizontal(|ui| {
                                base.render_default(ui);
                                if ui
                                    .small_button(icons::CARET_RIGHT)
                                    .on_hover_text(format!("Open record: {canonical}"))
                                    .clicked()
                                {
                                    clicks_inner.borrow_mut().push(canonical.clone());
                                }
                            });
                            return;
                        }
                    }
                }
                ctx.render_default(ui);
            })
            .show(ui);
    }
    if let Ok(mut taken) = Rc::try_unwrap(clicks_cell).map(|c| c.into_inner()) {
        clicks.append(&mut taken);
    }
}

/// Pull JSON-tree syntax colors from the active mtech theme so the JSON
/// blocks shift presets along with the rest of the admin console
/// instead of staying on egui_json_tree's hardcoded dark palette.
fn themed_json_visuals(ui: &Ui) -> JsonTreeVisuals {
    let mut highlight = theme::accent(ui);
    highlight = Color32::from_rgba_unmultiplied(
        highlight.r(),
        highlight.g(),
        highlight.b(),
        80,
    );
    JsonTreeVisuals {
        object_key_color: theme::info(ui),
        array_idx_color: theme::weak_text(ui),
        null_color: theme::accent_secondary(ui),
        bool_color: theme::accent_secondary(ui),
        number_color: theme::warn(ui),
        string_color: theme::success(ui),
        highlight_color: highlight,
        punctuation_color: theme::weak_text(ui),
    }
}

/// Walk an entry's args + result, return every distinct RecordID found.
fn extract_record_ids_from_entry(entry: &McpToolCallLog) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = parse_json(&entry.args_json) {
        collect_record_ids(&v, &mut out);
    }
    if let Some(body) = &entry.result_json {
        if let Some(v) = parse_json(body) {
            collect_record_ids(&v, &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn collect_record_ids(value: &JsonValue, out: &mut Vec<String>) {
    match value {
        JsonValue::String(s) => {
            if is_record_id_string(s) {
                out.push(canonical_record_id(s));
                return;
            }
            // MCP wraps the real payload as a stringified JSON document
            // under `content[*].text` — recurse so nested RecordIDs surface.
            if let Some(nested) = parse_json(s) {
                if nested != JsonValue::String(s.clone()) {
                    collect_record_ids(&nested, out);
                    return;
                }
            }
            // SQL queries and other free text mention RecordIDs inline
            // (e.g. `WHERE id = stress_test_run:` … ``) — scan for them.
            scan_string_for_record_ids(s, out);
        }
        JsonValue::Array(arr) => {
            for item in arr {
                collect_record_ids(item, out);
            }
        }
        JsonValue::Object(obj) => {
            for v in obj.values() {
                collect_record_ids(v, out);
            }
        }
        _ => {}
    }
}

/// Parse a SurrealDB record-id string into `(table, naked_key)`.
///
/// Accepts both `table:abc123` (bare key) and `` table:`abc-def` ``
/// (backtick-wrapped key — SurrealDB uses these whenever the key has
/// hyphens or other punctuation, which is exactly the shape that the
/// tool-call logs always carry: `` stress_test_run:`uuid-with-dashes` ``
/// , `` computer:`DESKTOP-3F0BA5T:f4ac11309` ``, etc.).
///
/// The returned key has any surrounding backticks stripped, because
/// that's the form `RecordId::new(table, key)` expects on the wire.
fn parse_record_id_components(s: &str) -> Option<(&str, &str)> {
    let (table, rest) = s.split_once(':')?;
    if !is_valid_table_ident(table) || rest.is_empty() {
        return None;
    }
    let key = if rest.len() >= 2 && rest.starts_with('`') && rest.ends_with('`') {
        &rest[1..rest.len() - 1]
    } else {
        rest
    };
    if key.is_empty() {
        return None;
    }
    if key.chars().any(char::is_whitespace) {
        return None;
    }
    if key.chars().all(|c| c.is_ascii_digit()) {
        // Avoid matching time strings like `12:34` or port numbers.
        return None;
    }
    Some((table, key))
}

fn is_valid_table_ident(table: &str) -> bool {
    let mut chars = table.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_record_id_string(s: &str) -> bool {
    parse_record_id_components(s).is_some()
}

/// Stripped form used as the cache key, breadcrumb id, and on-screen
/// label. Collapses `` table:`key` `` and `table:key` to the same form
/// so click-through dedupes cleanly.
fn canonical_record_id(s: &str) -> String {
    match parse_record_id_components(s) {
        Some((table, key)) => format!("{table}:{key}"),
        None => s.to_string(),
    }
}

/// Find every RecordID-shaped substring in arbitrary text (used for SQL
/// query args like `WHERE id = stress_test_run:` … `` and the like).
/// Honours word boundaries so `astress_test_run:abc` doesn't match.
fn scan_string_for_record_ids(s: &str, out: &mut Vec<String>) {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let prev_is_ident = i > 0 && is_ident_char(chars[i - 1].1);
        if prev_is_ident || !is_ident_start(chars[i].1) {
            i += 1;
            continue;
        }
        let start_byte = chars[i].0;
        let mut j = i;
        while j < n && is_ident_char(chars[j].1) {
            j += 1;
        }
        if j >= n || chars[j].1 != ':' {
            i = if j > i { j } else { i + 1 };
            continue;
        }
        let colon_idx = j;
        j += 1;
        let key_end_idx = if j < n && chars[j].1 == '`' {
            // Backtick-wrapped key — read until matching backtick.
            j += 1;
            while j < n && chars[j].1 != '`' {
                j += 1;
            }
            if j >= n {
                i = colon_idx + 1;
                continue;
            }
            j += 1;
            j
        } else {
            let key_start = j;
            while j < n
                && (is_ident_char(chars[j].1) || matches!(chars[j].1, '-' | '.'))
            {
                j += 1;
            }
            if j == key_start {
                i = colon_idx + 1;
                continue;
            }
            j
        };
        let end_byte = if key_end_idx >= n {
            s.len()
        } else {
            chars[key_end_idx].0
        };
        let matched = &s[start_byte..end_byte];
        if is_record_id_string(matched) {
            out.push(canonical_record_id(matched));
        }
        i = key_end_idx;
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_lowercase()
}

fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit()
}

fn parse_json(s: &str) -> Option<JsonValue> {
    serde_json::from_str(s).ok()
}

fn status_glyph(status: &McpToolCallStatus) -> (&'static str, Color32) {
    match status {
        McpToolCallStatus::Pending => (icons::STATUS_WAIT, Color32::from_rgb(255, 200, 80)),
        McpToolCallStatus::Success => (icons::STATUS_ON, Color32::from_rgb(120, 220, 140)),
        McpToolCallStatus::Error => (icons::STATUS_ERR, Color32::from_rgb(230, 120, 120)),
    }
}

fn short_id(s: &str) -> String {
    const MAX: usize = 24;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…", &s[..MAX])
    }
}

fn format_elapsed(ms: u128) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        let secs = ms / 1000;
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

fn arg_preview(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "{}" || trimmed == "null" {
        return String::new();
    }
    const MAX: usize = 140;
    let collapsed: String = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX {
        collapsed
    } else {
        let cut: String = collapsed.chars().take(MAX).collect();
        format!("{cut}…")
    }
}

fn format_entries_for_clipboard(entries: &[McpToolCallLog]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "=== MCP Tool Calls — {} entry/entries ===\n\n",
        entries.len()
    ));
    for (i, e) in entries.iter().enumerate() {
        let status = match e.status {
            McpToolCallStatus::Pending => "PENDING",
            McpToolCallStatus::Success => "OK",
            McpToolCallStatus::Error => "ERR",
        };
        out.push_str(&format!(
            "[{}/{}] {} {}::{}  req={}  elapsed={}\n",
            i + 1,
            entries.len(),
            status,
            e.plugin_id,
            e.tool_name,
            e.request_id,
            format_elapsed(e.elapsed_ms()),
        ));
        out.push_str("Arguments:\n");
        out.push_str(&pretty_json_or_raw(&e.args_json));
        out.push_str("\n\nResult:\n");
        match (&e.status, e.result_json.as_deref()) {
            (McpToolCallStatus::Pending, _) => out.push_str("(still pending)"),
            (_, Some(body)) => out.push_str(&pretty_json_or_raw(body)),
            (_, None) => out.push_str("(no result body)"),
        }
        out.push_str("\n\n");
    }
    out
}

fn pretty_json_or_raw(raw: &str) -> String {
    serde_json::from_str::<JsonValue>(raw)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| raw.to_string())
}
