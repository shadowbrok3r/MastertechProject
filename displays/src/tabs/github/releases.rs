//! Release browser for the Downloads page: every release, every asset, per-asset download progress.

use std::collections::HashMap;
use std::str::FromStr;

use chrono::DateTime;
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{
    Align, Button, Checkbox, ComboBox, Context, Frame, Layout, Margin, ProgressBar, RichText, ScrollArea,
    Spinner, Ui, Widget,
};
use reqwest::header::{HeaderName, ACCEPT, USER_AGENT};
use reqwest::Client;

use super::{Asset, GithubRelease, GIT_MASTER_TECH_REPO_BASE};
use crate::ui_tools::list_row::{Lead, ListRow};
use crate::ui_tools::{glass_card, icons, theme};
use crate::{PlatformSpawner, Spawner};

/// Releases requested per fetch.
const RELEASES_PER_PAGE: u32 = 50;
/// Width of the release list column.
const LIST_W: f32 = 260.0;
/// Below this width the release list collapses into a combo box.
const NARROW_W: f32 = 720.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    Windows,
    Linux,
    Unknown,
}

impl Platform {
    /// Platform of the machine running this UI.
    #[cfg(target_arch = "wasm32")]
    pub fn host() -> Self {
        let agent = web_sys::window()
            .and_then(|w| w.navigator().user_agent().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if agent.contains("linux") && !agent.contains("android") {
            Self::Linux
        } else {
            Self::Windows
        }
    }

    /// Platform of the machine running this UI.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn host() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Unknown
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::Unknown => "Other",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Windows => icons::WINDOWS_LOGO,
            Self::Linux => icons::LINUX_LOGO,
            Self::Unknown => icons::FILE,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Product {
    MasterTech,
    QcApp,
    Other,
}

impl Product {
    fn label(self) -> &'static str {
        match self {
            Self::MasterTech => "MasterTech",
            Self::QcApp => "QC App",
            Self::Other => "Other",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::MasterTech => "Technician client",
            Self::QcApp => "Build QC and provisioning",
            Self::Other => "Release file",
        }
    }
}

/// Product and platform inferred from an asset file name.
pub fn classify(asset_name: &str) -> (Product, Platform) {
    let name = asset_name.to_ascii_lowercase();
    let product = if name.starts_with("mastertech") {
        Product::MasterTech
    } else if name.starts_with("qc_app") || name.starts_with("qc-app") {
        Product::QcApp
    } else {
        Product::Other
    };
    let platform = if name.ends_with(".exe") || name.ends_with(".msi") {
        Platform::Windows
    } else if name.contains("linux") || name.ends_with(".appimage") || name.ends_with(".deb") {
        Platform::Linux
    } else {
        Platform::Unknown
    };
    (product, platform)
}

#[derive(Clone, Debug)]
enum DownloadState {
    Running { done: u64, total: u64 },
    Saved,
    Cancelled,
    Failed(String),
}

enum FetchState {
    Idle,
    Loading,
    Loaded,
    Failed(String),
}

enum Event {
    Releases(Result<Vec<GithubRelease>, String>),
    Download { key: String, state: DownloadState },
}

pub struct ReleaseBrowser {
    releases: Vec<GithubRelease>,
    fetch: FetchState,
    selected_tag: Option<String>,
    show_prereleases: bool,
    show_all_platforms: bool,
    downloads: HashMap<String, DownloadState>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Default for ReleaseBrowser {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            releases: Vec::new(),
            fetch: FetchState::Idle,
            selected_tag: None,
            show_prereleases: false,
            show_all_platforms: false,
            downloads: HashMap::new(),
            tx,
            rx,
        }
    }
}

impl ReleaseBrowser {
    /// Re-fetches the release list.
    pub fn refresh(&mut self, ctx: &Context) {
        if matches!(self.fetch, FetchState::Loading) {
            return;
        }
        self.fetch = FetchState::Loading;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        PlatformSpawner::spawn(async move {
            let result = fetch_releases().await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Releases(result));
            ctx.request_repaint();
        });
    }

    fn receive(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Releases(Ok(releases)) => {
                    self.releases = releases.into_iter().filter(|r| !r.draft).collect();
                    self.fetch = FetchState::Loaded;
                }
                Event::Releases(Err(e)) => {
                    log::error!("release fetch failed: {e}");
                    self.fetch = FetchState::Failed(e);
                }
                Event::Download { key, state } => {
                    self.downloads.insert(key, state);
                }
            }
        }
    }

    fn visible(&self) -> Vec<&GithubRelease> {
        self.releases
            .iter()
            .filter(|r| self.show_prereleases || !r.prerelease)
            .collect()
    }

    /// Tag of the newest non-prerelease.
    fn latest_tag(&self) -> Option<&str> {
        self.releases
            .iter()
            .find(|r| !r.prerelease)
            .map(|r| r.tag_name.as_str())
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.receive();
        if matches!(self.fetch, FetchState::Idle) {
            self.refresh(ui.ctx());
        }
        if self.downloads.values().any(|d| matches!(d, DownloadState::Running { .. })) {
            ui.ctx().request_repaint();
        }

        Frame::new().inner_margin(16.0).show(ui, |ui| {
            self.header(ui);
            ui.add_space(8.0);

            let visible_count = self.visible().len();
            match &self.fetch {
                FetchState::Loading | FetchState::Idle if self.releases.is_empty() => {
                    centered_message(ui, |ui| {
                        Spinner::new().size(18.0).ui(ui);
                        ui.label(RichText::new("Loading releases…").color(theme::weak_text(ui)));
                    });
                    return;
                }
                FetchState::Failed(e) if self.releases.is_empty() => {
                    let e = e.clone();
                    centered_message(ui, |ui| {
                        ui.label(icons::icon_colored(icons::STATUS_WARN, theme::error(ui)).size(24.0));
                        ui.label(RichText::new("Couldn't load releases").strong());
                        ui.label(RichText::new(e).small().color(theme::weak_text(ui)));
                    });
                    return;
                }
                _ if visible_count == 0 => {
                    centered_message(ui, |ui| {
                        ui.label(RichText::new("No releases published yet.").color(theme::weak_text(ui)));
                    });
                    return;
                }
                _ => {}
            }

            let selected_visible = self
                .selected_tag
                .as_deref()
                .is_some_and(|t| self.visible().iter().any(|r| r.tag_name == t));
            if !selected_visible {
                self.selected_tag = self.visible().first().map(|r| r.tag_name.clone());
            }

            if ui.available_width() < NARROW_W {
                self.release_combo(ui);
                ui.add_space(8.0);
                self.release_detail(ui);
            } else {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        [LIST_W, ui.available_height()].into(),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(LIST_W);
                            self.release_list(ui);
                        },
                    );
                    ui.separator();
                    ui.vertical(|ui| self.release_detail(ui));
                });
            }
        });
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(icons::icon_colored(icons::DOWNLOAD, theme::accent(ui)).size(22.0));
            ui.vertical(|ui| {
                ui.label(RichText::new("Downloads").heading().strong().color(theme::strong_text(ui)));
                let subtitle = match self.latest_tag() {
                    Some(tag) => format!("{} releases · latest {tag}", self.releases.len()),
                    None => "MasterTech and QC App releases".to_string(),
                };
                ui.label(RichText::new(subtitle).small().color(theme::weak_text(ui)));
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let loading = matches!(self.fetch, FetchState::Loading);
                let refresh = ui
                    .add_enabled(!loading, Button::new(format!("{} Refresh", icons::REFRESH)))
                    .on_hover_text("Fetch the release list again");
                if refresh.clicked() {
                    self.refresh(ui.ctx());
                }
                if loading && !self.releases.is_empty() {
                    Spinner::new().size(14.0).ui(ui);
                }
                Checkbox::new(&mut self.show_all_platforms, "All platforms").ui(ui);
                Checkbox::new(&mut self.show_prereleases, "Pre-releases").ui(ui);
            });
        });
        if let FetchState::Failed(e) = &self.fetch
            && !self.releases.is_empty()
        {
            ui.label(
                RichText::new(format!("{} Refresh failed: {e}", icons::STATUS_WARN))
                    .small()
                    .color(theme::error(ui)),
            );
        }
        glass_card::hairline(ui);
    }

    fn release_list(&mut self, ui: &mut Ui) {
        let latest = self.latest_tag().map(str::to_owned);
        let mut clicked = None;
        ScrollArea::vertical()
            .id_salt("release_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for release in self.visible() {
                    let is_latest = latest.as_deref() == Some(release.tag_name.as_str());
                    let detail = release_detail_line(release, is_latest);
                    let title = display_name(release);
                    let lead_color = is_latest.then(|| theme::success(ui));
                    let row = ListRow::new(title)
                        .lead(Lead::Icon(icons::TAG, lead_color))
                        .detail(&detail)
                        .selected(self.selected_tag.as_deref() == Some(release.tag_name.as_str()))
                        .show(ui);
                    if row.clicked() {
                        clicked = Some(release.tag_name.clone());
                    }
                }
            });
        if clicked.is_some() {
            self.selected_tag = clicked;
        }
    }

    fn release_combo(&mut self, ui: &mut Ui) {
        let latest = self.latest_tag().map(str::to_owned);
        let current = self.selected_tag.clone().unwrap_or_default();
        let mut picked = None;
        ComboBox::from_id_salt("release_combo")
            .width(ui.available_width())
            .selected_text(format!("{} {current}", icons::TAG))
            .show_ui(ui, |ui| {
                for release in self.visible() {
                    let is_latest = latest.as_deref() == Some(release.tag_name.as_str());
                    let text = format!("{}  ·  {}", display_name(release), release_detail_line(release, is_latest));
                    if ui.selectable_label(release.tag_name == current, text).clicked() {
                        picked = Some(release.tag_name.clone());
                    }
                }
            });
        if picked.is_some() {
            self.selected_tag = picked;
        }
    }

    fn release_detail(&mut self, ui: &mut Ui) {
        let Some(release) = self
            .selected_tag
            .as_deref()
            .and_then(|t| self.releases.iter().find(|r| r.tag_name == t))
            .cloned()
        else {
            return;
        };
        let is_latest = self.latest_tag() == Some(release.tag_name.as_str());
        let host = Platform::host();

        ScrollArea::vertical()
            .id_salt("release_detail")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(display_name(&release)).heading().strong().color(theme::strong_text(ui)));
                    if is_latest {
                        badge(ui, "Latest", theme::success(ui));
                    }
                    if release.prerelease {
                        badge(ui, "Pre-release", theme::warn(ui));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let weak = theme::weak_text(ui);
                    if release.name != release.tag_name && !release.name.is_empty() {
                        ui.label(RichText::new(&release.tag_name).small().color(weak));
                        ui.label(RichText::new("·").small().color(weak));
                    }
                    ui.label(RichText::new(format!("Published {}", format_date(release_date(&release)))).small().color(weak));
                    if !release.html_url.is_empty() {
                        ui.label(RichText::new("·").small().color(weak));
                        ui.hyperlink_to(
                            RichText::new(format!("{} View on GitHub", icons::GITHUB_LOGO)).small(),
                            &release.html_url,
                        );
                    }
                });
                ui.add_space(10.0);

                let mut assets: Vec<(Product, Platform, &Asset)> = release
                    .assets
                    .iter()
                    .map(|a| {
                        let (product, platform) = classify(&a.name);
                        (product, platform, a)
                    })
                    .collect();
                assets.sort_by_key(|(product, platform, a)| (*product, *platform != host, a.name.to_ascii_lowercase()));
                let hidden = assets
                    .iter()
                    .filter(|(_, p, _)| !self.show_all_platforms && *p != host && *p != Platform::Unknown)
                    .count();
                if !self.show_all_platforms {
                    assets.retain(|(_, p, _)| *p == host || *p == Platform::Unknown);
                }

                glass_card::titled_card(ui, icons::PACKAGE, "Files", Some(&format!("{} files", release.assets.len())), |ui| {
                    if assets.is_empty() {
                        ui.label(RichText::new("No files for this platform in this release.").color(theme::weak_text(ui)));
                    }
                    for (product, platform, asset) in &assets {
                        self.asset_row(ui, *product, *platform, asset, host);
                    }
                    if hidden > 0 {
                        let text = format!("{hidden} more for other platforms");
                        if ui.link(RichText::new(text).small()).clicked() {
                            self.show_all_platforms = true;
                        }
                    }
                });

                glass_card::titled_card(ui, icons::FILE_TEXT, "Release notes", None, |ui| {
                    if release.body.trim().is_empty() {
                        ui.label(RichText::new("No release notes.").color(theme::weak_text(ui)));
                    } else {
                        release_notes(ui, &release.body);
                    }
                });
            });
    }

    fn asset_row(&mut self, ui: &mut Ui, product: Product, platform: Platform, asset: &Asset, host: Platform) {
        let recommended = product == Product::MasterTech && platform == host;
        let key = asset.url.clone();
        glass_card::group(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let glyph_color = if recommended { theme::accent_secondary(ui) } else { theme::weak_text(ui) };
                ui.label(icons::icon_colored(platform.glyph(), glyph_color).size(22.0));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} for {}", product.label(), platform.label())).strong().color(theme::strong_text(ui)));
                        if recommended {
                            badge(ui, "Recommended", theme::accent_secondary(ui));
                        }
                    });
                    ui.label(
                        RichText::new(format!("{} · {} · {}", product.description(), asset.name, format_size(asset.size)))
                            .small()
                            .color(theme::weak_text(ui)),
                    );
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let state = self.downloads.get(&key).cloned();
                    match state {
                        Some(DownloadState::Running { done, total }) => {
                            let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                            ProgressBar::new(frac)
                                .desired_width(160.0)
                                .text(format!("{} / {}", format_size(done), format_size(total)))
                                .ui(ui);
                        }
                        other => {
                            let label = match other {
                                Some(DownloadState::Saved) => format!("{} Download again", icons::DOWNLOAD),
                                Some(DownloadState::Failed(_)) => format!("{} Retry", icons::REFRESH),
                                _ => format!("{} Download", icons::DOWNLOAD),
                            };
                            let button = if recommended {
                                Button::new(RichText::new(label).strong())
                            } else {
                                Button::new(label)
                            };
                            let enabled = cfg!(not(any(target_os = "ios", target_os = "android")));
                            if ui.add_enabled(enabled, button).clicked() {
                                self.start_download(ui.ctx(), asset.clone());
                            }
                            match other {
                                Some(DownloadState::Saved) => {
                                    ui.label(icons::icon_colored(icons::CHECK, theme::success(ui)))
                                        .on_hover_text("Saved");
                                }
                                Some(DownloadState::Failed(e)) => {
                                    ui.label(icons::icon_colored(icons::STATUS_WARN, theme::error(ui)))
                                        .on_hover_text(e);
                                }
                                _ => {}
                            }
                        }
                    }
                });
            });
        });
    }

    fn start_download(&mut self, ctx: &Context, asset: Asset) {
        let key = asset.url.clone();
        self.downloads.insert(key.clone(), DownloadState::Running { done: 0, total: asset.size });
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        PlatformSpawner::spawn(async move {
            let state = match download_asset(&asset, &key, &tx, &ctx).await {
                Ok(true) => DownloadState::Saved,
                Ok(false) => DownloadState::Cancelled,
                Err(e) => {
                    log::error!("download of {} failed: {e:#}", asset.name);
                    DownloadState::Failed(format!("{e:#}"))
                }
            };
            let _ = tx.send(Event::Download { key, state });
            ctx.request_repaint();
        });
    }
}

fn centered_message(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    ui.add_space(48.0);
    ui.vertical_centered(contents);
}

fn badge(ui: &mut Ui, text: &str, color: eframe::egui::Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke((1.0, color.gamma_multiply(0.6)))
        .corner_radius(6.0)
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().strong().color(color));
        });
}

/// Renders the markdown subset GitHub release notes use: headings, bullets, emphasis and bare URLs.
fn release_notes(ui: &mut Ui, body: &str) {
    for raw in body.lines() {
        let trimmed = raw.trim_start();
        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix('#') {
            let text = strip_emphasis(heading.trim_start_matches('#').trim());
            ui.add_space(4.0);
            ui.label(RichText::new(text).strong().color(theme::strong_text(ui)));
            continue;
        }
        let bullet = ["- ", "* ", "+ "].iter().find_map(|m| trimmed.strip_prefix(m));
        let indent = (raw.len() - trimmed.len()) as f32 * 4.0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let text = match bullet {
                Some(rest) => {
                    ui.add_space(8.0 + indent);
                    ui.label(RichText::new("•  ").color(theme::accent_secondary(ui)));
                    rest
                }
                None => trimmed,
            };
            inline_text(ui, &strip_emphasis(text));
        });
    }
}

/// Labels with bare `http(s)://` URLs as hyperlinks.
fn inline_text(ui: &mut Ui, text: &str) {
    let mut rest = text;
    while let Some(start) = rest.find("http://").or_else(|| rest.find("https://")) {
        if start > 0 {
            ui.label(&rest[..start]);
        }
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == ')' || c == ']')
            .unwrap_or(tail.len());
        let url = &tail[..end];
        ui.hyperlink_to(short_url(url), url);
        rest = &tail[end..];
    }
    if !rest.is_empty() {
        ui.label(rest);
    }
}

/// `owner/repo/pull/123` and `compare/a...b` links shortened to their last segment.
fn short_url(url: &str) -> String {
    let path = url.trim_end_matches('/');
    match path.rsplit_once('/') {
        Some((head, last)) if head.ends_with("/pull") || head.ends_with("/issues") => format!("#{last}"),
        Some((head, last)) if head.ends_with("/compare") => last.to_string(),
        _ => url.to_string(),
    }
}

/// Drops `**`, `__` and backticks; turns `[text](url)` into `text url`.
fn strip_emphasis(text: &str) -> String {
    let mut out = text.replace("**", "").replace("__", "").replace('`', "");
    while let Some(open) = out.find('[') {
        let Some(mid) = out[open..].find("](").map(|i| open + i) else { break };
        let Some(close) = out[mid..].find(')').map(|i| mid + i) else { break };
        let label = out[open + 1..mid].to_string();
        let url = out[mid + 2..close].to_string();
        out.replace_range(open..=close, &format!("{label} {url}"));
    }
    out
}

fn display_name(release: &GithubRelease) -> &str {
    if release.name.trim().is_empty() {
        &release.tag_name
    } else {
        &release.name
    }
}

fn release_date(release: &GithubRelease) -> &str {
    if release.published_at.is_empty() {
        &release.created_at
    } else {
        &release.published_at
    }
}

fn release_detail_line(release: &GithubRelease, is_latest: bool) -> String {
    let mut line = format_date(release_date(release));
    if is_latest {
        line.push_str(" · latest");
    }
    if release.prerelease {
        line.push_str(" · pre-release");
    }
    line
}

fn format_date(date: &str) -> String {
    DateTime::parse_from_rfc3339(date)
        .map(|d| d.format("%b %-d, %Y").to_string())
        .unwrap_or_else(|_| date.to_string())
}

fn format_size(bytes: u64) -> String {
    const MB: f64 = 1_048_576.0;
    let b = bytes as f64;
    if b >= MB {
        format!("{:.1} MB", b / MB)
    } else {
        format!("{:.0} KB", (b / 1024.0).max(1.0))
    }
}

async fn fetch_releases() -> anyhow::Result<Vec<GithubRelease>> {
    let releases = Client::new()
        .get(format!("{GIT_MASTER_TECH_REPO_BASE}/releases?per_page={RELEASES_PER_PAGE}"))
        .header(ACCEPT, "application/vnd.github+json")
        .header(HeaderName::from_str("X-GitHub-Api-Version")?, "2022-11-28")
        .header(USER_AGENT, "shadowbrok3r/Mastertech")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(releases)
}

/// Streams an asset into memory and writes it to a user-chosen file; `Ok(false)` when cancelled.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
async fn download_asset(asset: &Asset, key: &str, tx: &Sender<Event>, ctx: &Context) -> anyhow::Result<bool> {
    use futures::StreamExt;

    let Some(file) = rfd::AsyncFileDialog::new().set_file_name(&asset.name).save_file().await else {
        return Ok(false);
    };
    anyhow::ensure!(!asset.url.is_empty(), "asset has no download URL");

    let resp = Client::new()
        .get(super::proxied_github_asset_url(&asset.url))
        .header(ACCEPT, "application/octet-stream")
        .header(USER_AGENT, "shadowbrok3r/Mastertech")
        .header(HeaderName::from_str("X-GitHub-Api-Version")?, "2022-11-28")
        .send()
        .await?
        .error_for_status()?;

    let total = resp.content_length().filter(|n| *n > 0).unwrap_or(asset.size);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut stream = resp.bytes_stream();
    let mut last_reported = 0u64;
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk?);
        let done = bytes.len() as u64;
        // Progress at most every 512 KiB.
        if done - last_reported >= 512 * 1024 {
            last_reported = done;
            let _ = tx.send(Event::Download {
                key: key.to_string(),
                state: DownloadState::Running { done, total },
            });
            ctx.request_repaint();
        }
    }
    if total > 0 && bytes.len() as u64 != total {
        anyhow::bail!("incomplete download: {} of {} bytes", bytes.len(), total);
    }

    file.write(&bytes).await?;
    Ok(true)
}

#[cfg(any(target_os = "ios", target_os = "android"))]
async fn download_asset(_: &Asset, _: &str, _: &Sender<Event>, _: &Context) -> anyhow::Result<bool> {
    anyhow::bail!("downloads are not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_release_assets() {
        assert_eq!(classify("Mastertech.exe"), (Product::MasterTech, Platform::Windows));
        assert_eq!(classify("Mastertech-linux"), (Product::MasterTech, Platform::Linux));
        assert_eq!(classify("qc_app.exe"), (Product::QcApp, Platform::Windows));
        assert_eq!(classify("notes.txt"), (Product::Other, Platform::Unknown));
    }

    #[test]
    fn shortens_github_links() {
        assert_eq!(short_url("https://github.com/o/r/pull/312"), "#312");
        assert_eq!(short_url("https://github.com/o/r/compare/v4.8.5...v4.8.6"), "v4.8.5...v4.8.6");
        assert_eq!(strip_emphasis("**Full** [notes](https://x.y)"), "Full notes https://x.y");
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(52_428_800), "50.0 MB");
        assert_eq!(format_size(2048), "2 KB");
    }
}
