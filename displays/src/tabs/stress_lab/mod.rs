//! Stress Lab — browse `stress_test_run` history by hardware component, and
//! compare runs of one stressor across every part that has run it.

mod compare;
mod data;
mod metrics;
mod plots;

use std::collections::{BTreeMap, BTreeSet};

use crossbeam::channel::{Receiver, Sender, unbounded};
use database::schema::{
    Datetime, HardwareKind, RecordId, RecordIdExt, RunResult, StressTestEvent,
};
use eframe::egui::{Align, ComboBox, Layout, RichText, ScrollArea, TextEdit, Ui, vec2};

use crate::ui_tools::framed_controls::{FramedSelectable, selectable_card};
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};

use data::{ComponentInfo, RunRecord, SeriesBucket};
pub use metrics::{RunMetric, SeriesMetric};

/// Runs held in memory. The table is ~500 rows today; the whole lab filters,
/// sorts and aggregates from this one snapshot instead of re-querying.
const RUN_LIMIT: u64 = 4000;

/// Components charted side by side in Compare. Past this the chart is unreadable
/// and the telemetry fetch stops being cheap.
const MAX_COMPARE_SERIES: usize = 12;

/// Below this the three columns cannot hold a legible row, so they stack.
const STACK_BREAKPOINT: f32 = 900.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Browse,
    Compare,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultFilter {
    All,
    Pass,
    Fail,
    Aborted,
    Inconclusive,
    InProgress,
}

impl ResultFilter {
    const VALUES: [Self; 6] = [
        Self::All,
        Self::Pass,
        Self::Fail,
        Self::Aborted,
        Self::Inconclusive,
        Self::InProgress,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All results",
            Self::Pass => "Pass",
            Self::Fail => "Fail",
            Self::Aborted => "Aborted",
            Self::Inconclusive => "Inconclusive",
            Self::InProgress => "In progress",
        }
    }

    fn accepts(self, result: RunResult) -> bool {
        match self {
            Self::All => true,
            Self::Pass => result == RunResult::Pass,
            Self::Fail => result == RunResult::Fail,
            Self::Aborted => result == RunResult::Aborted,
            Self::Inconclusive => result == RunResult::Inconclusive,
            Self::InProgress => result == RunResult::InProgress,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunSort {
    Started,
    Result,
    Tool,
    Throughput,
    Temp,
    Duration,
}

impl RunSort {
    const VALUES: [Self; 6] = [
        Self::Started,
        Self::Result,
        Self::Tool,
        Self::Throughput,
        Self::Temp,
        Self::Duration,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Started => "Date",
            Self::Result => "Result",
            Self::Tool => "Stressor",
            Self::Throughput => "Peak throughput",
            Self::Temp => "Peak temp",
            Self::Duration => "Duration",
        }
    }
}

/// Severity order for sorting: the runs worth reading come first.
fn result_rank(result: RunResult) -> u8 {
    match result {
        RunResult::Fail => 0,
        RunResult::Aborted => 1,
        RunResult::Inconclusive => 2,
        RunResult::InProgress => 3,
        RunResult::Pass => 4,
    }
}

fn result_color(ui: &Ui, result: RunResult) -> eframe::egui::Color32 {
    match result {
        RunResult::Pass => theme::result_pass(ui),
        RunResult::Fail => theme::result_fail(ui),
        RunResult::Aborted => theme::result_aborted(ui),
        RunResult::Inconclusive => theme::result_inconclusive(ui),
        RunResult::InProgress => theme::info(ui),
    }
}

fn result_glyph(result: RunResult) -> &'static str {
    match result {
        RunResult::Pass => icons::p::CHECK_CIRCLE,
        RunResult::Fail => icons::p::X_CIRCLE,
        RunResult::Aborted => icons::p::PROHIBIT,
        RunResult::Inconclusive => icons::p::QUESTION,
        RunResult::InProgress => icons::p::HOURGLASS_SIMPLE,
    }
}

fn kind_glyph(kind: HardwareKind) -> &'static str {
    match kind {
        HardwareKind::Cpu => icons::p::CPU,
        HardwareKind::Gpu => icons::p::GRAPHICS_CARD,
        HardwareKind::RamModule | HardwareKind::RamKit => icons::p::MEMORY,
        HardwareKind::Ssd | HardwareKind::Hdd => icons::p::HARD_DRIVE,
        HardwareKind::Motherboard => icons::p::CIRCUITRY,
        HardwareKind::Psu => icons::p::LIGHTNING,
        HardwareKind::Cooler => icons::p::FAN,
    }
}

/// Series color for index `i`. The CVD-validated palette holds six; past that
/// the per-core palette takes over, which is legible but not CVD-validated —
/// hence the [`MAX_COMPARE_SERIES`] cap.
pub fn series_color(index: usize) -> eframe::egui::Color32 {
    theme::series_color(index).unwrap_or_else(|| theme::core_series_color(index))
}

/// What a component's run history adds up to. Derived from the run snapshot, so
/// it can never disagree with the list of runs the operator sees when they
/// click through.
#[derive(Default, Clone)]
pub struct ComponentStats {
    pub runs: u32,
    /// Runs where this part was the primary target. Compare attributes by
    /// target, so a part with runs but no targeted ones has nothing to chart.
    pub targeted_runs: u32,
    pub pass: u32,
    pub fail: u32,
    pub aborted: u32,
    pub inconclusive: u32,
    pub in_progress: u32,
    pub last_run: Option<Datetime>,
    pub best_throughput: Option<f64>,
    pub throughput_unit: Option<String>,
    pub peak_temp_c: Option<f64>,
    pub tools: BTreeSet<String>,
}

/// Which request an error or a loading flag belongs to. Untagged, one
/// channel's success cleared another's banner and one channel's failure
/// cleared another's in-flight guard.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Components,
    Runs,
    Detail,
    Compare,
}

/// Loaded telemetry for one run, keyed so a late reply for an abandoned
/// selection is discarded rather than drawn.
struct Detail {
    run: RecordId,
    buckets: Vec<SeriesBucket>,
    events: Vec<StressTestEvent>,
    bucket_secs: u32,
}

pub struct StressLab {
    components_tx: Sender<Vec<ComponentInfo>>,
    components_rx: Receiver<Vec<ComponentInfo>>,
    runs_tx: Sender<Vec<RunRecord>>,
    runs_rx: Receiver<Vec<RunRecord>>,
    detail_tx: Sender<(RecordId, u32, Vec<SeriesBucket>, Vec<StressTestEvent>)>,
    detail_rx: Receiver<(RecordId, u32, Vec<SeriesBucket>, Vec<StressTestEvent>)>,
    compare_tx: Sender<(u64, u32, Vec<SeriesBucket>)>,
    compare_rx: Receiver<(u64, u32, Vec<SeriesBucket>)>,
    error_tx: Sender<(Source, String)>,
    error_rx: Receiver<(Source, String)>,

    components: Vec<ComponentInfo>,
    runs: Vec<RunRecord>,
    stats: BTreeMap<String, ComponentStats>,
    /// Tool filter the cached `stats` were built under; a change rebuilds them
    /// so a card's counts always describe the runs its column is showing.
    stats_tool: Option<String>,
    tools: Vec<String>,

    view: View,
    kind_filter: Option<HardwareKind>,
    tool_filter: Option<String>,
    result_filter: ResultFilter,
    run_sort: RunSort,
    sort_desc: bool,
    search: String,
    only_with_runs: bool,
    interactive_charts: bool,

    selected_component: Option<RecordId>,
    detail: Option<Detail>,
    selected_run: Option<RecordId>,

    compare: compare::CompareState,
    compare_request: u64,

    /// Whether a fetch has been attempted, as distinct from whether it returned
    /// anything. Without it an empty result — or a failing DB — re-fires the
    /// query every frame, since `ui` asks for a load on each pass.
    components_requested: bool,
    runs_requested: bool,
    loading_components: bool,
    loading_runs: bool,
    loading_detail: bool,
    loading_compare: bool,
    errors: BTreeMap<Source, String>,
}

impl Default for StressLab {
    fn default() -> Self {
        let (components_tx, components_rx) = unbounded();
        let (runs_tx, runs_rx) = unbounded();
        let (detail_tx, detail_rx) = unbounded();
        let (compare_tx, compare_rx) = unbounded();
        let (error_tx, error_rx) = unbounded();
        Self {
            components_tx,
            components_rx,
            runs_tx,
            runs_rx,
            detail_tx,
            detail_rx,
            compare_tx,
            compare_rx,
            error_tx,
            error_rx,
            components: Vec::new(),
            runs: Vec::new(),
            stats: BTreeMap::new(),
            stats_tool: None,
            tools: Vec::new(),
            view: View::Browse,
            kind_filter: None,
            tool_filter: None,
            result_filter: ResultFilter::All,
            run_sort: RunSort::Started,
            sort_desc: true,
            search: String::new(),
            only_with_runs: true,
            interactive_charts: false,
            selected_component: None,
            detail: None,
            selected_run: None,
            compare: compare::CompareState::default(),
            compare_request: 0,
            components_requested: false,
            runs_requested: false,
            loading_components: false,
            loading_runs: false,
            loading_detail: false,
            loading_compare: false,
            errors: BTreeMap::new(),
        }
    }
}

impl StressLab {
    pub fn ui(&mut self, ui: &mut Ui) {
        // Loading from `ui` rather than only from the dock's tab-click hook:
        // MasterTech's `TabViewer` has no `on_tab_button` arm for this tab, so
        // on the desktop the lab never populated.
        self.refresh_on_open();
        self.poll_channels();
        if self.stats_tool != self.tool_filter {
            self.rebuild_derived();
        }
        self.toolbar(ui);
        for err in self.errors.values() {
            ui.colored_label(theme::error(ui), err);
        }
        ui.separator();

        match self.view {
            View::Browse => self.browse_ui(ui),
            View::Compare => {
                ScrollArea::vertical()
                    .id_salt("stress_lab_compare_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        compare::ui(self, ui);
                    });
            }
        }
    }

    pub fn refresh_on_open(&mut self) {
        if !self.components_requested {
            self.reload_components();
        }
        if !self.runs_requested {
            self.reload_runs();
        }
    }

    // ---- data ------------------------------------------------------------

    fn poll_channels(&mut self) {
        while let Ok(rows) = self.components_rx.try_recv() {
            self.components = rows;
            self.loading_components = false;
            self.errors.remove(&Source::Components);
        }
        while let Ok(rows) = self.runs_rx.try_recv() {
            self.runs = rows;
            self.loading_runs = false;
            self.errors.remove(&Source::Runs);
            self.rebuild_derived();
        }
        while let Ok((run, bucket_secs, buckets, events)) = self.detail_rx.try_recv() {
            // The flag clears on any reply; only the payload is gated on the
            // selection still being the one that asked for it.
            self.loading_detail = false;
            self.errors.remove(&Source::Detail);
            if self.selected_run.as_ref() == Some(&run) {
                self.detail = Some(Detail {
                    run,
                    buckets,
                    events,
                    bucket_secs,
                });
            }
        }
        while let Ok((request, bucket_secs, buckets)) = self.compare_rx.try_recv() {
            self.loading_compare = false;
            self.errors.remove(&Source::Compare);
            if request == self.compare_request {
                self.compare.buckets = buckets;
                self.compare.bucket_secs = bucket_secs;
            }
        }
        while let Ok((source, err)) = self.error_rx.try_recv() {
            match source {
                Source::Components => self.loading_components = false,
                Source::Runs => self.loading_runs = false,
                Source::Detail => self.loading_detail = false,
                Source::Compare => self.loading_compare = false,
            }
            self.errors.insert(source, err);
        }
    }

    /// Per-component roll-ups and the stressor list, recomputed whenever the run
    /// snapshot changes. `hardware_test_baseline` is deliberately not used: its
    /// `math::sum` columns are floats that no typed read accepts, and its
    /// temperature columns aggregate a field only 8 of 522 runs populate.
    fn rebuild_derived(&mut self) {
        let mut stats: BTreeMap<String, ComponentStats> = BTreeMap::new();
        let mut tools: BTreeSet<String> = BTreeSet::new();
        self.stats_tool = self.tool_filter.clone();

        for run in &self.runs {
            // The stressor list stays whole; only the counts narrow, so
            // filtering never hides the option that would widen it again.
            tools.insert(run.tool_label.clone());
            if self
                .tool_filter
                .as_ref()
                .is_some_and(|t| &run.tool_label != t)
            {
                continue;
            }

            let mut touched: Vec<String> = run
                .touched_components
                .iter()
                .map(|c| c.key_string())
                .collect();
            if let Some(target) = &run.target_component {
                touched.push(target.key_string());
            }
            touched.sort();
            touched.dedup();

            let target_key = run.target_component.as_ref().map(|c| c.key_string());
            for key in touched {
                let entry = stats.entry(key.clone()).or_default();
                entry.runs += 1;
                if target_key.as_deref() == Some(key.as_str()) {
                    entry.targeted_runs += 1;
                }
                match run.result {
                    RunResult::Pass => entry.pass += 1,
                    RunResult::Fail => entry.fail += 1,
                    RunResult::Aborted => entry.aborted += 1,
                    RunResult::Inconclusive => entry.inconclusive += 1,
                    RunResult::InProgress => entry.in_progress += 1,
                }
                entry.tools.insert(run.tool_label.clone());
                if entry
                    .last_run
                    .is_none_or(|last| run.started_at.timestamp() > last.timestamp())
                {
                    entry.last_run = Some(run.started_at);
                }
                if let Some(tp) = run.peak_throughput() {
                    if entry.best_throughput.is_none_or(|best| tp > best) {
                        entry.best_throughput = Some(tp);
                        entry.throughput_unit = run.throughput_unit().map(str::to_owned);
                    }
                }
                if let Some(temp) = run.peak_temp_c() {
                    if entry.peak_temp_c.is_none_or(|peak| temp > peak) {
                        entry.peak_temp_c = Some(temp);
                    }
                }
            }
        }

        // Which tools each part has ever run is what `visible_components`
        // filters on, so it is collected from the unfiltered set.
        for run in &self.runs {
            let mut touched: Vec<String> = run
                .touched_components
                .iter()
                .map(|c| c.key_string())
                .collect();
            if let Some(target) = &run.target_component {
                touched.push(target.key_string());
            }
            for key in touched {
                stats
                    .entry(key)
                    .or_default()
                    .tools
                    .insert(run.tool_label.clone());
            }
        }

        self.stats = stats;
        self.tools = tools.into_iter().collect();
        if let Some(tool) = &self.tool_filter {
            if !self.tools.iter().any(|t| t == tool) {
                self.tool_filter = None;
            }
        }
        if self.compare.tool.is_none() {
            self.compare.tool = self.busiest_tool();
        }
    }

    /// The stressor with the most runs — the useful default for Compare.
    fn busiest_tool(&self) -> Option<String> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for run in &self.runs {
            *counts.entry(run.tool_label.as_str()).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(tool, _)| tool.to_string())
    }

    fn reload_components(&mut self) {
        self.components_requested = true;
        self.loading_components = true;
        let tx = self.components_tx.clone();
        let etx = self.error_tx.clone();
        PlatformSpawner::spawn(async move {
            match data::fetch_components().await {
                Ok(rows) => {
                    let _ = tx.send(rows);
                }
                Err(e) => {
                    log::warn!("stress_lab components fetch: {e}");
                    let _ = etx.send((Source::Components, format!("Hardware catalog: {e}")));
                }
            }
        });
    }

    fn reload_runs(&mut self) {
        self.runs_requested = true;
        self.loading_runs = true;
        let tx = self.runs_tx.clone();
        let etx = self.error_tx.clone();
        PlatformSpawner::spawn(async move {
            match data::fetch_runs(RUN_LIMIT).await {
                Ok(rows) => {
                    let _ = tx.send(rows);
                }
                Err(e) => {
                    log::warn!("stress_lab runs fetch: {e}");
                    let _ = etx.send((Source::Runs, format!("Run history: {e}")));
                }
            }
        });
    }

    fn refresh_all(&mut self) {
        self.errors.clear();
        self.reload_components();
        self.reload_runs();
        if let Some(run) = self.selected_run.clone() {
            self.load_detail(&run);
        }
        self.compare.dirty = true;
    }

    /// One tick per second nominal, so the bucket is sized to keep any run under
    /// a few hundred points regardless of how long it ran. An unknown duration
    /// falls back to a bucket wide enough that even a day-long run stays
    /// bounded — 1-second buckets on a run of unknown length is how the query
    /// ends up asking for tens of thousands of rows.
    fn bucket_for(duration_secs: Option<f64>) -> u32 {
        const UNKNOWN_DURATION_BUCKET: u32 = 30;
        match duration_secs {
            Some(secs) if secs > 0.0 => ((secs / 400.0).ceil() as u32).max(1),
            _ => UNKNOWN_DURATION_BUCKET,
        }
    }

    fn load_detail(&mut self, run_id: &RecordId) {
        let duration = self
            .runs
            .iter()
            .find(|r| &r.id == run_id)
            .and_then(|r| r.span_secs());
        let bucket_secs = Self::bucket_for(duration);
        self.loading_detail = true;
        let tx = self.detail_tx.clone();
        let etx = self.error_tx.clone();
        let run = run_id.clone();
        PlatformSpawner::spawn(async move {
            let buckets = match data::fetch_series(std::slice::from_ref(&run), bucket_secs).await {
                Ok(rows) => rows,
                Err(e) => {
                    log::warn!("stress_lab detail metrics: {e}");
                    let _ = etx.send((Source::Detail, format!("Run telemetry: {e}")));
                    Vec::new()
                }
            };
            let events = data::fetch_events(&run).await.unwrap_or_default();
            let _ = tx.send((run, bucket_secs, buckets, events));
        });
    }

    fn select_run(&mut self, run_id: &RecordId) {
        self.selected_run = Some(run_id.clone());
        self.detail = None;
        self.load_detail(run_id);
    }

    // ---- derived views ---------------------------------------------------

    fn component_stats(&self, id: &RecordId) -> ComponentStats {
        self.stats.get(&id.key_string()).cloned().unwrap_or_default()
    }

    /// Parts Compare can actually chart: ones that were the primary target of
    /// at least one run, most-tested first. Offering a part that only ever
    /// appeared in `touched_components` would open an empty chart.
    pub fn tested_components(&self) -> Vec<(RecordId, String)> {
        let mut out: Vec<(RecordId, String, u32)> = self
            .components
            .iter()
            .filter_map(|c| {
                let stats = self.component_stats(&c.id);
                (stats.targeted_runs > 0).then(|| {
                    (c.id.clone(), c.display_name.clone(), stats.targeted_runs)
                })
            })
            .collect();
        out.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
        out.into_iter().map(|(id, name, _)| (id, name)).collect()
    }

    fn component_name(&self, id: &RecordId) -> String {
        self.components
            .iter()
            .find(|c| &c.id == id)
            .map(|c| c.display_name.clone())
            .unwrap_or_else(|| id.key_string())
    }

    fn visible_components(&self) -> Vec<&ComponentInfo> {
        let needle = self.search.trim().to_ascii_lowercase();
        let mut out: Vec<&ComponentInfo> = self
            .components
            .iter()
            .filter(|c| self.kind_filter.is_none_or(|k| c.kind == k))
            .filter(|c| {
                let stats = self.component_stats(&c.id);
                if self.only_with_runs && stats.runs == 0 {
                    return false;
                }
                match &self.tool_filter {
                    Some(tool) => stats.tools.contains(tool),
                    None => true,
                }
            })
            .filter(|c| {
                needle.is_empty()
                    || c.display_name.to_ascii_lowercase().contains(&needle)
                    || c.vendor.to_ascii_lowercase().contains(&needle)
                    || c.id.key_string().contains(&needle)
            })
            .collect();
        out.sort_by(|a, b| {
            let (sa, sb) = (self.component_stats(&a.id), self.component_stats(&b.id));
            sb.fail
                .cmp(&sa.fail)
                .then(sb.runs.cmp(&sa.runs))
                .then_with(|| a.display_name.cmp(&b.display_name))
        });
        out
    }

    fn visible_runs(&self) -> Vec<&RunRecord> {
        let needle = self.search.trim().to_ascii_lowercase();
        let mut out: Vec<&RunRecord> = self
            .runs
            .iter()
            .filter(|r| {
                self.selected_component
                    .as_ref()
                    .is_none_or(|c| r.involves(c))
            })
            .filter(|r| self.tool_filter.as_ref().is_none_or(|t| &r.tool_label == t))
            .filter(|r| self.result_filter.accepts(r.result))
            .filter(|r| {
                self.kind_filter.is_none_or(|kind| {
                    self.components.iter().any(|c| {
                        c.kind == kind
                            && (r.target_component.as_ref() == Some(&c.id)
                                || r.touched_components.contains(&c.id))
                    })
                })
            })
            .filter(|r| needle.is_empty() || self.run_matches(r, &needle))
            .collect();

        out.sort_by(|a, b| {
            let ord = match self.run_sort {
                RunSort::Started => a.started_at.timestamp().cmp(&b.started_at.timestamp()),
                RunSort::Result => result_rank(a.result)
                    .cmp(&result_rank(b.result))
                    .reverse()
                    .then(a.started_at.timestamp().cmp(&b.started_at.timestamp())),
                RunSort::Tool => a
                    .tool_label
                    .cmp(&b.tool_label)
                    .reverse()
                    .then(a.started_at.timestamp().cmp(&b.started_at.timestamp())),
                RunSort::Throughput => cmp_opt(a.peak_throughput(), b.peak_throughput()),
                RunSort::Temp => cmp_opt(a.peak_temp_c(), b.peak_temp_c()),
                RunSort::Duration => cmp_opt(a.duration_actual_secs, b.duration_actual_secs),
            };
            if self.sort_desc { ord.reverse() } else { ord }
        });
        out
    }

    fn run_matches(&self, run: &RunRecord, needle: &str) -> bool {
        run.tool_label.to_ascii_lowercase().contains(needle)
            || run.failure_kind.to_ascii_lowercase().contains(needle)
            || run.result.as_str().contains(needle)
            || run.id.key_string().contains(needle)
            || run
                .preset_label
                .as_ref()
                .is_some_and(|p| p.to_ascii_lowercase().contains(needle))
            || run
                .hostname
                .as_ref()
                .is_some_and(|h| h.to_ascii_lowercase().contains(needle))
            || run
                .target_component
                .as_ref()
                .is_some_and(|c| self.component_name(c).to_ascii_lowercase().contains(needle))
    }

    // ---- chrome ----------------------------------------------------------

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .framed_selectable_label(self.view == View::Browse, format!("{} Browse", icons::LIST))
                .clicked()
            {
                self.view = View::Browse;
            }
            if ui
                .framed_selectable_label(
                    self.view == View::Compare,
                    format!("{} Compare", icons::p::CHART_LINE),
                )
                .clicked()
            {
                self.view = View::Compare;
            }
            ui.separator();
            if ui.button(format!("{} Refresh", icons::REFRESH)).clicked() {
                self.refresh_all();
            }

            let prev_kind = self.kind_filter;
            ComboBox::from_id_salt("stress_lab_kind_filter")
                .selected_text(match self.kind_filter {
                    Some(kind) => kind.as_str(),
                    None => "All kinds",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.kind_filter, None, "All kinds");
                    for kind in [
                        HardwareKind::Cpu,
                        HardwareKind::Gpu,
                        HardwareKind::RamKit,
                        HardwareKind::RamModule,
                        HardwareKind::Ssd,
                        HardwareKind::Hdd,
                        HardwareKind::Motherboard,
                        HardwareKind::Psu,
                        HardwareKind::Cooler,
                    ] {
                        ui.selectable_value(&mut self.kind_filter, Some(kind), kind.as_str());
                    }
                });
            if self.kind_filter != prev_kind {
                self.selected_component = None;
                self.compare.dirty = true;
            }

            ComboBox::from_id_salt("stress_lab_tool_filter")
                .selected_text(match &self.tool_filter {
                    Some(tool) => tool.as_str(),
                    None => "All stressors",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.tool_filter, None, "All stressors");
                    for tool in self.tools.clone() {
                        ui.selectable_value(&mut self.tool_filter, Some(tool.clone()), tool);
                    }
                });

            ui.add(
                TextEdit::singleline(&mut self.search)
                    .hint_text(format!("{} Search runs & hardware", icons::SEARCH))
                    .desired_width(220.0),
            );
            if !self.search.is_empty() && ui.button(icons::CLOSE).clicked() {
                self.search.clear();
            }

            ui.checkbox(&mut self.only_with_runs, "Tested hardware only");
            ui.checkbox(&mut self.interactive_charts, "Pan/zoom charts");

            if self.loading_components
                || self.loading_runs
                || self.loading_detail
                || self.loading_compare
            {
                ui.spinner();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} runs · {} parts",
                        self.runs.len(),
                        self.stats.len()
                    ))
                    .weak(),
                );
            });
        });
    }

    fn browse_ui(&mut self, ui: &mut Ui) {
        let full_w = ui.available_width();
        let height = ui.available_height();

        if full_w < STACK_BREAKPOINT {
            ScrollArea::vertical()
                .id_salt("stress_lab_stacked")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.components_panel(ui);
                    ui.separator();
                    self.runs_panel(ui);
                    ui.separator();
                    self.detail_panel(ui);
                });
            return;
        }

        let hardware_w = (full_w * 0.26).clamp(220.0, 360.0);
        let runs_w = (full_w * 0.30).clamp(240.0, 420.0);
        ui.horizontal_top(|ui| {
            column(ui, hardware_w, height, "stress_lab_hardware_col", |ui| {
                self.components_panel(ui)
            });
            ui.separator();
            column(ui, runs_w, height, "stress_lab_runs_col", |ui| {
                self.runs_panel(ui)
            });
            ui.separator();
            let rest = ui.available_width().max(240.0);
            column(ui, rest, height, "stress_lab_detail_col", |ui| {
                self.detail_panel(ui)
            });
        });
    }

    fn components_panel(&mut self, ui: &mut Ui) {
        let visible = self
            .visible_components()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        ui.horizontal(|ui| {
            ui.heading("Hardware");
            ui.label(RichText::new(format!("{}", visible.len())).weak());
        });

        if self.selected_component.is_some() && ui.button("Clear selection").clicked() {
            self.selected_component = None;
        }

        if visible.is_empty() {
            ui.label(RichText::new("No hardware matches the filters.").weak());
            return;
        }

        for component in visible {
            let selected = self.selected_component.as_ref() == Some(&component.id);
            let stats = self.component_stats(&component.id);
            let response = selectable_card(ui, ("hw", component.id.key_string()), selected, |ui| {
                ui.label(RichText::new(format!(
                    "{} {}",
                    kind_glyph(component.kind),
                    component.display_name
                )));
                ui.label(
                    RichText::new(format!("{} · {}", component.kind.as_str(), component.id.key_string()))
                        .weak()
                        .small(),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{} runs", stats.runs)).small());
                    if stats.pass > 0 {
                        ui.label(
                            RichText::new(format!("{} pass", stats.pass))
                                .color(theme::result_pass(ui))
                                .small(),
                        );
                    }
                    if stats.fail > 0 {
                        ui.label(
                            RichText::new(format!("{} fail", stats.fail))
                                .color(theme::result_fail(ui))
                                .small(),
                        );
                    }
                    if stats.aborted > 0 {
                        ui.label(
                            RichText::new(format!("{} aborted", stats.aborted))
                                .color(theme::result_aborted(ui))
                                .small(),
                        );
                    }
                    if let Some(temp) = stats.peak_temp_c {
                        ui.label(RichText::new(format!("{temp:.0}°C peak")).small());
                    }
                });
            })
            .response;

            if response.clicked() {
                self.selected_component = if selected {
                    None
                } else {
                    Some(component.id.clone())
                };
                if let Some(id) = self.selected_component.clone() {
                    self.compare.component = Some(id);
                    self.compare.dirty = true;
                }
                self.selected_run = None;
                self.detail = None;
                self.loading_detail = false;
            }
            ui.add_space(3.0);
        }
    }

    fn runs_panel(&mut self, ui: &mut Ui) {
        let visible: Vec<RunRecord> = self.visible_runs().into_iter().cloned().collect();

        ui.horizontal(|ui| {
            ui.heading("Runs");
            ui.label(RichText::new(format!("{}", visible.len())).weak());
        });
        ui.horizontal_wrapped(|ui| {
            ComboBox::from_id_salt("stress_lab_result_filter")
                .width(126.0)
                .selected_text(self.result_filter.label())
                .show_ui(ui, |ui| {
                    for filter in ResultFilter::VALUES {
                        ui.selectable_value(&mut self.result_filter, filter, filter.label());
                    }
                });
            ComboBox::from_id_salt("stress_lab_sort")
                .width(140.0)
                .selected_text(format!("Sort: {}", self.run_sort.label()))
                .show_ui(ui, |ui| {
                    for sort in RunSort::VALUES {
                        ui.selectable_value(&mut self.run_sort, sort, sort.label());
                    }
                });
            let glyph = if self.sort_desc {
                icons::p::SORT_DESCENDING
            } else {
                icons::p::SORT_ASCENDING
            };
            if ui.button(glyph).on_hover_text("Reverse sort").clicked() {
                self.sort_desc = !self.sort_desc;
            }
        });
        if let Some(component) = self.selected_component.clone() {
            ui.label(
                RichText::new(format!("Filtered to {}", self.component_name(&component)))
                    .weak()
                    .small(),
            );
        }

        if visible.is_empty() {
            ui.label(RichText::new("No runs match the filters.").weak());
            return;
        }

        for run in visible {
            let selected = self.selected_run.as_ref() == Some(&run.id);
            let response = selectable_card(ui, ("run", run.id.key_string()), selected, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            result_glyph(run.result),
                            run.result.as_str()
                        ))
                        .color(result_color(ui, run.result))
                        .strong(),
                    );
                    ui.label(RichText::new(fmt_datetime(&run.started_at)).weak().small());
                });
                ui.label(RichText::new(format!(
                    "{} · {}",
                    run.tool_label,
                    run.preset_label.as_deref().unwrap_or("—")
                )));
                ui.horizontal_wrapped(|ui| {
                    if let Some(host) = &run.hostname {
                        ui.label(RichText::new(host).weak().small());
                    }
                    if let Some(target) = &run.target_component {
                        ui.label(
                            RichText::new(self.component_name(target))
                                .weak()
                                .small(),
                        );
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if let (Some(tp), Some(unit)) = (run.peak_throughput(), run.throughput_unit())
                    {
                        ui.label(RichText::new(format!("peak {tp:.1} {unit}")).small());
                    }
                    if let Some(temp) = run.peak_temp_c() {
                        ui.label(RichText::new(format!("{temp:.0}°C")).small());
                    }
                    if let Some(secs) = run.duration_actual_secs {
                        ui.label(RichText::new(fmt_duration(secs)).small());
                    }
                    if run.failure_kind != "none" {
                        ui.label(
                            RichText::new(&run.failure_kind)
                                .color(result_color(ui, run.result))
                                .small(),
                        );
                    }
                });
            })
            .response;

            if response.clicked() {
                self.select_run(&run.id);
            }
            ui.add_space(3.0);
        }
    }

    fn detail_panel(&mut self, ui: &mut Ui) {
        ui.heading("Run detail");
        let Some(run_id) = self.selected_run.clone() else {
            ui.label(RichText::new("Select a run to view its telemetry and events.").weak());
            return;
        };

        if let Some(run) = self.runs.iter().find(|r| r.id == run_id).cloned() {
            ui.label(
                RichText::new(format!("{} · {}", run.tool_label, run.result.as_str()))
                    .color(result_color(ui, run.result))
                    .strong(),
            );
            ui.label(RichText::new(run.id.key_string()).weak().small());
            if let Some(target) = &run.target_component {
                ui.label(format!(
                    "Target: {} ({})",
                    self.component_name(target),
                    run.target_kind.as_str()
                ));
            }
            if let Some(preset) = &run.preset_label {
                ui.label(format!("Preset: {preset}"));
            }
            ui.horizontal_wrapped(|ui| {
                if let Some(secs) = run.duration_actual_secs {
                    ui.label(fmt_duration(secs));
                }
                if let Some(host) = &run.hostname {
                    ui.label(host);
                }
                if run.failure_kind != "none" {
                    ui.label(
                        RichText::new(format!("failure: {}", run.failure_kind))
                            .color(theme::result_fail(ui)),
                    );
                }
            });
        }

        ui.separator();
        match &self.detail {
            Some(detail) if detail.run == run_id => {
                plots::run_detail(ui, detail.bucket_secs, &detail.buckets, self.interactive_charts);
                ui.separator();
                plots::events(ui, &detail.events);
            }
            _ => {
                ui.label(RichText::new("Loading telemetry…").weak());
            }
        }
    }
}

/// A fixed-width, independently scrolling column.
///
/// Every column needs its own `id_salt`: sibling `Ui`s built by `ui.vertical`
/// share one `Ui::id`, so bare `ScrollArea::vertical()` calls resolve to the
/// same key and share one scroll offset.
fn column<R>(ui: &mut Ui, width: f32, height: f32, id: &str, add: impl FnOnce(&mut Ui) -> R) {
    ui.allocate_ui_with_layout(vec2(width, height), Layout::top_down(Align::Min), |ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .show(ui, add);
    });
}

fn cmp_opt(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub fn fmt_datetime(dt: &Datetime) -> String {
    chrono::DateTime::<chrono::Utc>::from(*dt)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// Legend-safe run stamp: unique down to the second, unlike [`fmt_datetime`].
pub fn fmt_run_stamp(dt: &Datetime) -> String {
    chrono::DateTime::<chrono::Utc>::from(*dt)
        .format("%m-%d %H:%M:%S")
        .to_string()
}

pub fn fmt_duration(secs: f64) -> String {
    if secs >= 3600.0 {
        format!("{:.1}h", secs / 3600.0)
    } else if secs >= 60.0 {
        format!("{:.0}m", secs / 60.0)
    } else {
        format!("{secs:.0}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_keeps_any_run_under_a_few_hundred_points() {
        // An unknown duration must not fall back to 1-second buckets.
        assert_eq!(StressLab::bucket_for(None), 30);
        assert_eq!(StressLab::bucket_for(Some(0.0)), 30);
        assert_eq!(StressLab::bucket_for(Some(60.0)), 1);
        assert_eq!(StressLab::bucket_for(Some(400.0)), 1);
        assert_eq!(StressLab::bucket_for(Some(1800.0)), 5);
        // 38k ticks, the longest run in the corpus.
        assert_eq!(StressLab::bucket_for(Some(38_400.0)), 96);
    }

    #[test]
    fn failures_sort_ahead_of_passes() {
        assert!(result_rank(RunResult::Fail) < result_rank(RunResult::Aborted));
        assert!(result_rank(RunResult::Aborted) < result_rank(RunResult::Inconclusive));
        assert!(result_rank(RunResult::Inconclusive) < result_rank(RunResult::Pass));
    }

    #[test]
    fn missing_values_sort_below_present_ones() {
        assert_eq!(cmp_opt(Some(1.0), None), std::cmp::Ordering::Greater);
        assert_eq!(cmp_opt(None, Some(1.0)), std::cmp::Ordering::Less);
        assert_eq!(cmp_opt(None, None), std::cmp::Ordering::Equal);
    }
}
