//! Title-strip geometry across presets and densities, and its headless preview renders.

use eframe::egui::{
    CentralPanel, Event, Modifiers, PointerButton, Pos2, Rect, Ui, epaint::ClippedPrimitive, vec2,
};
use mtech_ui::title_bar::{Side, TitleBar};

use super::{
    controls::{self, ButtonKind, IconBox, IconButton, MenuButton, Metrics, TextButton, Tokens},
    headless_render::Headless,
    icons::svg,
    metrics::Density,
    theme_config::{MATCHABLE_PRESETS, PresetStyles},
};

/// Window width below which the sample strip collapses labels, as MasterTech's does.
const COMPACT_BELOW: f32 = 1280.0;

/// What the sample strip laid out.
struct SampleBar {
    /// Every control that must sit on the strip's centre line.
    controls: Vec<Rect>,
    account: Rect,
    strip: Rect,
}

/// A stand-in for MasterTech's title strip built from the same kit calls.
fn sample_title_bar(ui: &mut Ui, frameless: bool) -> SampleBar {
    let compact = ui.ctx().content_rect().width() < COMPACT_BELOW;
    let title = if frameless { "MasterTech 4.8.12" } else { "" };
    let mut bar = TitleBar::new("preview.title_bar", title).frameless(frameless);
    if frameless {
        bar = bar.mark(svg::MT_LOGO);
    }
    let mut rects = Vec::new();
    let mut account = Rect::NOTHING;
    let mut search = String::new();
    let out = bar.show(ui, |ui, side| {
        let t = Tokens::get(ui.ctx());
        let m = Metrics::get(ui.ctx());
        match side {
            Side::Left => {
                let view = MenuButton::new("View").icon(svg::TABS).show(ui, |_| ());
                let mode = MenuButton::new("Tune-up / QC")
                    .icon(svg::WRENCH)
                    .tint(t.primary)
                    .show(ui, |_| ());
                let layout = MenuButton::new(if compact { "" } else { "Layout" })
                    .icon(svg::LAYOUT)
                    .tip("UI layout")
                    .show(ui, |_| ());
                controls::vline(ui, m.control_h - 8.0);
                let new = TextButton::new(if compact { "New" } else { "New task" })
                    .icon(svg::PLUS)
                    .kind(ButtonKind::Primary)
                    .show(ui);
                let width = (ui.available_width() - 140.0).clamp(140.0, 260.0);
                let field = controls::search_field(
                    ui,
                    &mut search,
                    "Search tasks, customers, phone",
                    width,
                );
                let semantic =
                    controls::icon_button(ui, svg::SPARKLE, IconBox::Md, false, "Semantic search");
                rects.extend([
                    view.0.rect,
                    mode.0.rect,
                    layout.0.rect,
                    new.rect,
                    field.rect,
                    semantic.rect,
                ]);
            }
            Side::Right => {
                let (user, _) = MenuButton::new("logan")
                    .icon(svg::USER_CIRCLE)
                    .keep_open(true)
                    .show(ui, sample_account_menu);
                account = user.rect;
                controls::vline(ui, m.control_h - 8.0);
                let bell = IconButton::new(svg::BELL_RINGING, IconBox::Md)
                    .tint(t.error)
                    .tip("3 unread alerts")
                    .show(ui);
                controls::count_badge(ui, bell.rect, 3, t.error);
                let moon = IconButton::new(svg::MOON, IconBox::Md)
                    .selected(true)
                    .tip("Do Not Disturb")
                    .show(ui);
                controls::count_badge(ui, moon.rect, 12, t.secondary);
                let ask = if compact {
                    controls::icon_button(ui, svg::ROBOT, IconBox::Md, false, "Ask")
                } else {
                    TextButton::new("Ask")
                        .icon(svg::ROBOT)
                        .kind(ButtonKind::Ghost)
                        .show(ui)
                };
                rects.extend([user.rect, bell.rect, moon.rect, ask.rect]);
                if !compact {
                    let chip = controls::pill(ui, svg::DESKTOP_TOWER, "DESKTOP-3C0R0D7", false);
                    rects.push(chip.rect);
                }
            }
        }
    });
    SampleBar {
        controls: rects,
        account,
        strip: out.rect,
    }
}

/// The account menu MasterTech opens from its user button.
fn sample_account_menu(ui: &mut Ui) {
    let m = Metrics::get(ui.ctx());
    ui.set_width(270.0);
    let row = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
        ui.horizontal(|ui| {
            ui.set_min_height(m.row_h);
            ui.add_space(m.pad.x);
            add(ui);
        });
    };
    row(ui, &mut |ui| {
        let _ = controls::icon_label(ui, svg::IDENTIFICATION_BADGE, "logan");
    });
    row(ui, &mut |ui| {
        let _ = controls::icon_label(ui, svg::DESKTOP_TOWER, "DESKTOP-3C0R0D7");
    });
    let mut store = 7_u64;
    row(ui, &mut |ui| {
        let _ = controls::icon_label(ui, svg::STOREFRONT, "Store");
        ui.with_layout(
            eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
            |ui| {
                ui.add_space(m.pad.x);
                let _ = controls::dropdown(
                    ui,
                    "preview.store",
                    &mut store,
                    &[(7, "RIV"), (8, "LTN")],
                    96.0,
                );
            },
        );
    });
    let rule = |ui: &mut Ui| {
        ui.add_space(m.gap.y / 2.0);
        controls::hairline(ui);
        ui.add_space(m.gap.y / 2.0);
    };
    rule(ui);
    let _ = controls::menu_item(ui, svg::GEAR, "Preferences");
    let _ = controls::menu_item(ui, svg::DOWNLOAD_SIMPLE, "Downloads");
    let _ = controls::menu_item(ui, svg::PALETTE, "Theme");
    let _ = controls::menu_item(ui, svg::ARROW_CLOCKWISE, "Refresh data");
    rule(ui);
    let _ = controls::menu_item(ui, svg::CLOUD_ARROW_DOWN, "Check for updates");
    let mut auto = true;
    row(ui, &mut |ui| {
        let _ = controls::toggle(ui, &mut auto, "Install updates automatically");
    });
    let mut custom = true;
    row(ui, &mut |ui| {
        let _ = controls::toggle(ui, &mut custom, "MasterTech title bar");
    });
    let _ = controls::menu_item(ui, svg::TERMINAL_WINDOW, "Restart in terminal mode");
    let _ = controls::menu_check_item(ui, svg::SQUARES_FOUR, "Everything", true);
    rule(ui);
    let _ = controls::menu_item(ui, svg::SIGN_OUT, "Log out");
}

/// The sample strip over an empty panel, as the window's root Ui lays it out.
fn sample_window(ui: &mut Ui, frameless: bool) -> SampleBar {
    let bar = sample_title_bar(ui, frameless);
    CentralPanel::default().show(ui, |_| ());
    bar
}

#[test]
fn title_strip_controls_share_the_control_height_and_the_strip_centre() {
    let mut h = Headless::new(vec2(1440.0, 120.0), 1.0);
    for preset in MATCHABLE_PRESETS {
        for density in Density::ALL {
            h.theme(preset, density);
            let mut bar = None;
            for _ in 0..2 {
                bar = Some(h.frame_root(Vec::new(), |ui| sample_window(ui, true)).0);
            }
            let bar = bar.expect("a pass ran");
            let m = Metrics::for_density(density);
            assert_eq!(bar.strip.height(), m.bar_h, "{}", preset.as_str());
            for (i, r) in bar.controls.iter().enumerate() {
                assert_eq!(
                    (r.height(), r.center().y),
                    (m.control_h, m.bar_h / 2.0),
                    "{} at {density:?}: control {i} is {r:?}",
                    preset.as_str()
                );
            }
        }
    }
}

fn press(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::default(),
    }
}

/// Frames that click the account trigger; the primitives of the last.
fn open_account_menu(h: &mut Headless) -> Vec<ClippedPrimitive> {
    let mut bar = None;
    for _ in 0..3 {
        bar = Some(h.frame_root(Vec::new(), |ui| sample_window(ui, true)).0);
    }
    let at = bar.expect("a pass ran").account.center();
    for events in [
        vec![Event::PointerMoved(at)],
        vec![press(at, true)],
        vec![press(at, false)],
        vec![Event::PointerMoved(at + vec2(0.0, 200.0))],
    ] {
        h.frame_root(events, |ui| sample_window(ui, true));
    }
    let mut prims = Vec::new();
    for _ in 0..3 {
        prims = h.frame_root(Vec::new(), |ui| sample_window(ui, true)).1;
    }
    prims
}

/// Writes title-strip previews; run with `-- --ignored render_title_bar_previews`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_title_bar_previews() {
    let dir = std::env::var_os("MTECH_RENDER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mtech-renders"));
    std::fs::create_dir_all(&dir).expect("render directory");
    let ppp = std::env::var("MTECH_RENDER_PPP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0_f32);
    for (name, preset) in [
        ("mtech_studio", PresetStyles::MtechStudio),
        ("mtech_glass", PresetStyles::MtechGlassFull),
        ("shipped_classic", PresetStyles::ShippedClassic),
    ] {
        for (shape, width, frameless) in [
            ("wide", 1440.0, true),
            ("compact", 1000.0, true),
            ("system", 1000.0, false),
        ] {
            let mut h = Headless::new(vec2(width, 52.0), ppp);
            h.theme(preset, Density::Regular);
            let mut prims = Vec::new();
            for _ in 0..4 {
                prims = h
                    .frame_root(Vec::new(), |ui| sample_window(ui, frameless))
                    .1;
            }
            h.image(&prims)
                .save(dir.join(format!("{name}_title_{shape}.png")))
                .expect("write title png");
        }
        let mut h = Headless::new(vec2(1000.0, 420.0), ppp);
        h.theme(preset, Density::Regular);
        let prims = open_account_menu(&mut h);
        h.image(&prims)
            .save(dir.join(format!("{name}_title_menu.png")))
            .expect("write menu png");
    }
}
