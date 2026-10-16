//! Control-kit geometry across every preset and density, and the headless preview renders.

use eframe::egui::{Id, Rect, Ui, Vec2, WidgetText, vec2};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use super::{
    controls::{self, IconBox, Metrics},
    dock_style,
    headless_render::Headless,
    icons::svg::{self, Icon},
    metrics::Density,
    theme_config::{MATCHABLE_PRESETS, PresetStyles, ThemeConfig},
};

/// Controls in [`kit_row`].
const KIT_ROW_LEN: usize = 10;

/// Rects of one row holding every control that must line up.
fn kit_row(ui: &mut Ui) -> Vec<Rect> {
    let mut text = "Text".to_owned();
    let mut size = 20.0_f32;
    let mut choice = 0_usize;
    let mut unset = 9_usize;
    let mut on = true;
    let mut segment = 0_u8;
    ui.horizontal(|ui| {
        vec![
            controls::button(ui, "Apply").rect,
            controls::icon_button(ui, svg::GEAR, IconBox::Md, false, "Settings").rect,
            controls::text_field(ui, &mut text, "Name", 120.0).rect,
            controls::value_field(ui, &mut size, 1.0..=100.0, "px", 70.0).rect,
            controls::dropdown(ui, "kit_row", &mut choice, &[(0, "One"), (1, "Two")], 100.0).rect,
            controls::toggle(ui, &mut on, "Live").rect,
            controls::Dropdown::new("kit_row_unset", &mut unset, &[(0, "One")])
                .placeholder("Pick")
                .width(80.0)
                .show(ui)
                .rect,
            controls::Slider::new(&mut size, 1.0..=100.0)
                .width(80.0)
                .show(ui)
                .rect,
            controls::segmented(
                ui,
                &mut segment,
                &[
                    controls::Segment::new(0, "A"),
                    controls::Segment::new(1, "B"),
                ],
            )
            .rect,
            controls::ProgressBar::new(0.5).width(60.0).show(ui).rect,
        ]
    })
    .inner
}

/// Why `rects` do not share `height` and one centre line, if they do not.
fn misalignment(rects: &[Rect], height: f32) -> Option<String> {
    let first = rects.first()?;
    rects.iter().enumerate().find_map(|(i, r)| {
        let tall = (r.height() - height).abs() > 0.01;
        let off = (r.center().y - first.center().y).abs() > 0.01;
        (tall || off).then(|| {
            format!(
                "control {i} is {r:?}, want height {height} centred on {}",
                first.center().y
            )
        })
    })
}

#[test]
fn kit_controls_share_the_control_height_and_centre_for_every_preset_and_density() {
    let mut h = Headless::new(vec2(1200.0, 200.0), 1.0);
    for preset in MATCHABLE_PRESETS {
        for density in Density::ALL {
            h.theme(preset, density);
            let rects = h.render(2, kit_row).0;
            let height = Metrics::for_density(density).control_h;
            assert_eq!(rects.len(), KIT_ROW_LEN);
            if let Some(why) = misalignment(&rects, height) {
                panic!("{} at {density:?}: {why}", preset.as_str());
            }
        }
    }
}

#[test]
fn the_geometry_check_fails_a_row_with_a_stock_or_shifted_control() {
    let mut h = Headless::new(vec2(1200.0, 200.0), 1.0);
    h.theme(PresetStyles::ShippedClassic, Density::Comfortable);
    let height = Metrics::for_density(Density::Comfortable).control_h;
    let (rects, _) = h.render(2, |ui| {
        let mut rects = kit_row(ui);
        rects.push(ui.horizontal(|ui| ui.label("stock label").rect).inner);
        rects
    });
    assert!(misalignment(&rects[..KIT_ROW_LEN], height).is_none());
    assert!(
        misalignment(&rects, height).is_some(),
        "a stock label passed as a kit control"
    );
    let mut shifted = rects[..KIT_ROW_LEN].to_vec();
    shifted[3] = shifted[3].translate(Vec2::new(0.0, 0.5));
    assert!(
        misalignment(&shifted, height).is_some(),
        "a half-point shift passed"
    );
}

/// A dock of sample tabs for previewing the dock chrome.
struct SampleTabs;

impl TabViewer for SampleTabs {
    type Tab = (Icon, &'static str);

    fn id(&mut self, tab: &mut Self::Tab) -> Id {
        Id::unique(tab.1)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        controls::tab_title(tab.0, tab.1)
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        controls::bar(ui, controls::BarKind::Header, |ui| {
            controls::icon_label(ui, tab.0, tab.1);
        });
        ui.add_space(8.0);
        controls::empty_state(
            ui,
            svg::MT_INTAKE,
            "Nothing here yet",
            "Tab content previews here.",
            &[],
        );
    }
}

fn dock(ui: &mut Ui) {
    let mut state = DockState::new(vec![
        (svg::LIST_CHECKS, "My Tasks"),
        (svg::STOREFRONT, "Store Tasks"),
        (svg::SPARKLE, "Ai"),
    ]);
    let surface = state.main_surface_mut();
    let [_, right] = surface.split_right(
        NodeIndex::root(),
        0.55,
        vec![
            (svg::GAUGE, "Resource Monitor"),
            (svg::MT_STRESS, "Stress Test"),
        ],
    );
    surface.split_below(right, 0.5, vec![(svg::SHIELD_CHECK, "Admin Console")]);
    DockArea::new(&mut state)
        .id(Id::unique("kit_preview_dock"))
        .style(dock_style::style(ui.ctx()))
        .show_close_buttons(true)
        .show_add_buttons(true)
        .show_inside(ui, &mut SampleTabs);
}

/// Writes the gallery and dock previews; run with `-- --ignored render_kit_previews`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_kit_previews() {
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
        let mut h = Headless::new(vec2(760.0, 3200.0), ppp);
        h.theme(preset, Density::Regular);
        let (end, image) = h.render(4, |ui| {
            controls::gallery(ui);
            ui.cursor().top()
        });
        let bottom = ((end + 16.0) * ppp).ceil().min(image.height() as f32) as u32;
        let image = image::imageops::crop_imm(&image, 0, 0, image.width(), bottom).to_image();
        image
            .save(dir.join(format!("{name}.png")))
            .expect("write gallery png");

        let mut h = Headless::new(vec2(900.0, 520.0), ppp);
        h.theme(preset, Density::Regular);
        let (_, image) = h.render(4, dock);
        image
            .save(dir.join(format!("{name}_dock.png")))
            .expect("write dock png");

        let mut h = Headless::new(vec2(860.0, 900.0), ppp);
        h.theme(preset, Density::Regular);
        let mut editor = ThemeConfig::default();
        let (tx, _rx) = crossbeam::channel::unbounded();
        let (_, image) = h.render(4, |ui| {
            let ctx = ui.ctx().clone();
            let _ = editor.edit_ui(ui, &ctx, tx.clone());
        });
        image
            .save(dir.join(format!("{name}_editor.png")))
            .expect("write editor png");
    }
}

/// Width of one preset tile in the preset sheets.
const TILE_W: f32 = 900.0;
/// Height of one preset tile in the preset sheets.
const TILE_H: f32 = 330.0;
/// Tiles per row and rows per preset sheet.
const SHEET_GRID: (u32, u32) = (3, 3);

/// One preset's sample of kit controls, labels, a section and menu rows.
fn preset_sample(ui: &mut Ui, preset: PresetStyles) {
    let t = controls::Tokens::get(ui.ctx());
    controls::heading(ui, svg::PALETTE, preset.as_str());
    kit_row(ui);
    ui.horizontal(|ui| {
        for kind in [
            controls::ButtonKind::Neutral,
            controls::ButtonKind::Primary,
            controls::ButtonKind::Danger,
            controls::ButtonKind::Ghost,
        ] {
            let _ = controls::TextButton::new("Save")
                .icon(svg::FLOPPY_DISK)
                .kind(kind)
                .show(ui);
        }
        let _ = controls::TextButton::new("Off").enabled(false).show(ui);
        controls::badge(ui, "Passed", t.success);
        controls::badge(ui, "Failed", t.error);
    });
    ui.horizontal(|ui| {
        controls::status_label(ui, svg::CHECK_CIRCLE, "Drivers current", t.success);
        controls::status_label(ui, svg::WARNING, "BIOS is old", t.warn);
        controls::IconLabel::new(svg::CPU, "Ryzen 9")
            .strong(true)
            .show(ui);
        controls::IconLabel::new(svg::PLUGS_CONNECTED, "relay://PC")
            .mono(true)
            .show(ui);
        controls::loading_row(ui, "Loading");
    });
    let mut text = String::from("Disabled");
    controls::TextField::new(&mut text)
        .icon(svg::USER)
        .enabled(false)
        .show(ui);
    let count = String::from("(3)");
    controls::Section::new(("sheet", preset.as_str()), "Hardware inventory")
        .icon(svg::CIRCUITRY)
        .title_tone(t.secondary)
        .icon_tone(t.secondary)
        .suffix(&count, None)
        .trailing(|ui| {
            controls::IconLabel::new(svg::MT_STRESS, "1 run in progress")
                .tone(t.warn)
                .show(ui);
        })
        .default_open(false)
        .header(ui);
    eframe::egui::Frame::menu(ui.style()).show(ui, |ui| {
        controls::menu_body(ui, |ui| {
            let _ = controls::menu_check_item(ui, svg::LIST_CHECKS, "My Tasks", true);
            let _ = controls::MenuItem::new("Archive 0 idle")
                .icon(svg::ARCHIVE)
                .enabled(false)
                .show(ui);
            let _ = controls::menu_danger_item(ui, svg::TRASH, "Delete file");
        });
    });
}

/// Writes every matchable preset's sample into sheets of tiles; run with `-- --ignored render_preset_sheets`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_preset_sheets() {
    let dir = std::env::var_os("MTECH_RENDER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mtech-renders"));
    std::fs::create_dir_all(&dir).expect("render directory");
    let (cols, rows) = SHEET_GRID;
    let per_sheet = (cols * rows) as usize;
    for (sheet, presets) in MATCHABLE_PRESETS.chunks(per_sheet).enumerate() {
        let mut out = image::RgbaImage::new(cols * TILE_W as u32, rows * TILE_H as u32);
        for (i, preset) in presets.iter().enumerate() {
            let mut h = Headless::new(vec2(TILE_W, TILE_H), 1.0);
            h.theme(*preset, Density::Regular);
            let (_, tile) = h.render(4, |ui| preset_sample(ui, *preset));
            let x = (i as u32 % cols) * TILE_W as u32;
            let y = (i as u32 / cols) * TILE_H as u32;
            image::imageops::replace(&mut out, &tile, x as i64, y as i64);
        }
        out.save(dir.join(format!("presets_{}.png", sheet + 1)))
            .expect("write preset sheet");
    }
}
