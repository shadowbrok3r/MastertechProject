//! Control-kit geometry across every preset and density, and the headless preview renders.

use eframe::egui::{
    Button, ComboBox, DragValue, Event, Id, Rect, Stroke, TextEdit, Ui, Vec2, WidgetText,
    class::HasClasses as _, vec2,
};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use super::{
    controls::{self, ButtonKind, IconBox, Metrics},
    dock_style,
    headless_render::Headless,
    icons::svg::{self, Icon},
    metrics::Density,
    theme_config::{self, MATCHABLE_PRESETS, PresetStyles, ThemeConfig},
    widget_theme,
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

/// Runs `passes` frames of `f` without rasterising; returns the last result.
fn settle<R>(h: &mut Headless, passes: usize, mut f: impl FnMut(&mut Ui) -> R) -> R {
    let mut last = None;
    for _ in 0..passes {
        last = Some(h.frame(Vec::new(), &mut f).0);
    }
    last.expect("at least one pass")
}

/// Rects of a stock Button, ComboBox, DragValue, TextEdit and checkbox in one row.
fn stock_row(ui: &mut Ui) -> Vec<Rect> {
    let mut text = "Text".to_owned();
    let mut size = 20.0_f32;
    let mut choice = 0_usize;
    let mut on = true;
    ui.horizontal(|ui| {
        let combo = ComboBox::from_id_salt("stock_row")
            .width(100.0)
            .selected_text("One")
            .show_ui(ui, |ui| {
                let _ = ui.selectable_value(&mut choice, 0, "One");
            });
        vec![
            ui.button("Apply").rect,
            combo.response.rect,
            ui.add(DragValue::new(&mut size)).rect,
            ui.add(TextEdit::singleline(&mut text).desired_width(120.0))
                .rect,
            ui.checkbox(&mut on, "Live").rect,
        ]
    })
    .inner
}

/// Rects of a stock and a kit "Apply" button, then a prominent stock and kit primary "Save".
fn button_row(ui: &mut Ui) -> [Rect; 4] {
    ui.horizontal(|ui| {
        let prominent = Button::new("Save")
            .with_class(widget_theme::PRIMARY)
            .with_class(widget_theme::PROMINENT);
        [
            ui.button("Apply").rect,
            controls::button(ui, "Apply").rect,
            ui.add(prominent).rect,
            controls::primary_button(ui, "Save").rect,
        ]
    })
    .inner
}

/// Whether `a` and `b` share a height and are within a point of the same width.
fn same_size(a: Rect, b: Rect) -> bool {
    a.height() == b.height() && (a.width() - b.width()).abs() <= 1.0
}

#[test]
fn stock_buttons_take_kit_button_geometry_for_every_preset_and_density() {
    let mut h = Headless::new(vec2(1200.0, 200.0), 1.0);
    for preset in MATCHABLE_PRESETS {
        for density in Density::ALL {
            h.theme(preset, density);
            let [stock, kit, prominent, primary] = settle(&mut h, 2, button_row);
            let at = format!("{} at {density:?}", preset.as_str());
            if h.ctx.global_style().visuals.button_frame {
                assert!(same_size(stock, kit), "{at}: {stock:?} vs {kit:?}");
            } else {
                widget_theme::set_enabled(&h.ctx, false);
                let [plain, ..] = settle(&mut h, 2, button_row);
                widget_theme::set_enabled(&h.ctx, true);
                assert_eq!(
                    stock, plain,
                    "{at}: a frameless theme keeps plain buttons stock"
                );
            }
            assert!(
                same_size(prominent, primary),
                "{at}: {prominent:?} vs {primary:?}"
            );
        }
    }
}

#[test]
fn stock_row_controls_share_the_control_height_and_centre_for_every_preset_and_density() {
    let mut h = Headless::new(vec2(1200.0, 200.0), 1.0);
    for preset in MATCHABLE_PRESETS {
        for density in Density::ALL {
            h.theme(preset, density);
            let rects = settle(&mut h, 2, stock_row);
            let height = Metrics::for_density(density).control_h;
            assert_eq!(rects.len(), 5);
            if let Some(why) = misalignment(&rects, height) {
                panic!("{} at {density:?}: {why}", preset.as_str());
            }
        }
    }
}

/// Rects of the kit controls that wrap or sit beside stock egui widgets.
fn kit_fields(ui: &mut Ui) -> Vec<Rect> {
    let mut name = String::new();
    let mut text = "Text".to_owned();
    let mut notes = "Notes".to_owned();
    let mut size = 20.0_f32;
    let mut choice = 0_usize;
    let mut rects = ui
        .horizontal(|ui| {
            vec![
                controls::button(ui, "Apply").rect,
                controls::TextButton::new("Save")
                    .kind(ButtonKind::Primary)
                    .show(ui)
                    .rect,
                controls::text_field(ui, &mut name, "Name", 120.0).rect,
                controls::text_field(ui, &mut text, "", 120.0).rect,
                controls::value_field(ui, &mut size, 1.0..=100.0, "px", 70.0).rect,
                controls::dropdown(
                    ui,
                    "kit_fields",
                    &mut choice,
                    &[(0, "One"), (1, "Two")],
                    100.0,
                )
                .rect,
            ]
        })
        .inner;
    rects.push(
        controls::TextArea::new(&mut notes)
            .rows(2)
            .width(300.0)
            .show(ui)
            .rect,
    );
    rects
}

/// The rects `kit_fields` draws, its centre pixels, and the whole image.
fn kit_fields_look(h: &mut Headless) -> (Vec<Rect>, Vec<image::Rgba<u8>>, image::RgbaImage) {
    let (rects, image) = h.render(2, kit_fields);
    let centres = rects
        .iter()
        .map(|r| *image.get_pixel(r.center().x as u32, r.center().y as u32))
        .collect();
    (rects, centres, image)
}

#[test]
fn kit_controls_look_the_same_with_the_stock_widget_theme_on_and_off() {
    let mut h = Headless::new(vec2(900.0, 160.0), 1.0);
    for preset in MATCHABLE_PRESETS {
        for density in Density::ALL {
            h.theme(preset, density);
            widget_theme::set_enabled(&h.ctx, true);
            let (on_rects, on_centres, on_image) = kit_fields_look(&mut h);
            widget_theme::set_enabled(&h.ctx, false);
            let (off_rects, off_centres, off_image) = kit_fields_look(&mut h);
            widget_theme::set_enabled(&h.ctx, true);
            let at = format!("{} at {density:?}", preset.as_str());
            assert_eq!(on_rects, off_rects, "{at}");
            assert_eq!(on_centres, off_centres, "{at}");
            assert!(
                on_image == off_image,
                "{at}: the kit row repaints differently"
            );
        }
    }
}

#[test]
fn the_look_check_sees_stock_widgets_follow_the_switch() {
    let mut h = Headless::new(vec2(900.0, 160.0), 1.0);
    h.theme(PresetStyles::ShippedClassic, Density::Regular);
    let draw = |h: &mut Headless| {
        h.render(2, |ui| {
            let _ = ui.button("Apply");
        })
        .1
    };
    let on = draw(&mut h);
    widget_theme::set_enabled(&h.ctx, false);
    let off = draw(&mut h);
    widget_theme::set_enabled(&h.ctx, true);
    assert!(on != off, "a stock button repaints with the switch");
}

/// A stock button in a Ui that overrides only the inactive border, as the task due-date button does.
fn due_date_button(ui: &mut Ui) -> (Rect, bool) {
    ui.scope(|ui| {
        let edge = ui.visuals().warn_fg_color;
        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(0.5, edge);
        let response = ui.button("10/17");
        (response.rect, response.hovered())
    })
    .inner
}

/// The due-date button's rect at rest and its rect and hover flag with the pointer on it.
fn due_date_rest_and_hover(h: &mut Headless) -> (Rect, (Rect, bool)) {
    let (rest, _) = settle(h, 2, due_date_button);
    let _ = h.frame(vec![Event::PointerMoved(rest.center())], due_date_button);
    let hovered = settle(h, 2, due_date_button);
    let _ = h.frame(vec![Event::PointerGone], due_date_button);
    (rest, hovered)
}

#[test]
fn a_one_state_override_keeps_a_stock_button_stock_on_hover_for_every_preset_and_density() {
    let mut h = Headless::new(vec2(400.0, 120.0), 1.0);
    for preset in MATCHABLE_PRESETS {
        for density in Density::ALL {
            h.theme(preset, density);
            let on = due_date_rest_and_hover(&mut h);
            widget_theme::set_enabled(&h.ctx, false);
            let off = due_date_rest_and_hover(&mut h);
            widget_theme::set_enabled(&h.ctx, true);
            let at = format!("{} at {density:?}", preset.as_str());
            assert!(on.1.1, "{at}: the pointer hovers the button");
            assert_eq!(on, off, "{at}: rest and hover match egui's own button");
        }
    }
}

#[test]
fn a_style_installed_mid_pass_keeps_later_stock_buttons_kit_for_every_preset() {
    let mut h = Headless::new(vec2(600.0, 160.0), 1.0);
    for preset in MATCHABLE_PRESETS {
        h.theme(preset, Density::Regular);
        let steady = settle(&mut h, 2, |ui| ui.button("Apply").rect);
        let (before, after) = settle(&mut h, 3, |ui| {
            let before = ui.button("Apply").rect;
            let mut style = (*ui.ctx().global_style()).clone();
            style.visuals.widgets.inactive.bg_stroke.width += 1.0;
            theme_config::install_style(ui.ctx(), style);
            (before, ui.button("Apply").rect)
        });
        let at = preset.as_str();
        assert_eq!(before.size(), steady.size(), "{at}");
        assert_eq!(
            after.size(),
            steady.size(),
            "{at}: a button after the install"
        );
    }
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

/// `MTECH_RENDER_DIR`, or a temp folder, created if missing.
fn render_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("MTECH_RENDER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mtech-renders"));
    std::fs::create_dir_all(&dir).expect("render directory");
    dir
}

/// Writes the gallery and dock previews; run with `-- --ignored render_kit_previews`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_kit_previews() {
    let dir = render_dir();
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

/// Writes the gallery's stock widgets section with the widget theme on and off; run with `-- --ignored render_stock_widgets`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_stock_widgets() {
    let dir = render_dir();
    for (name, preset) in [
        ("mtech_studio", PresetStyles::MtechStudio),
        ("mtech_glass", PresetStyles::MtechGlassFull),
        ("shipped_classic", PresetStyles::ShippedClassic),
    ] {
        for (switch, on) in [("on", true), ("off", false)] {
            let mut h = Headless::new(vec2(760.0, 900.0), 1.0);
            h.theme(preset, Density::Regular);
            widget_theme::set_enabled(&h.ctx, on);
            let (end, image) = h.render(4, |ui| {
                controls::stock_gallery(ui);
                ui.cursor().top()
            });
            let bottom = (end + 16.0).ceil().min(image.height() as f32) as u32;
            let image = image::imageops::crop_imm(&image, 0, 0, image.width(), bottom).to_image();
            image
                .save(dir.join(format!("stock_{name}_{switch}.png")))
                .expect("write stock widgets png");
        }
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
    let dir = render_dir();
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
