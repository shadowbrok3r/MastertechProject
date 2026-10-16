use super::*;
use crate::metrics::{self, Density};
use eframe::egui::{
    CentralPanel, Context, Event, FontDefinitions, Modifiers, PointerButton, Pos2, RawInput, Style,
    Vec2, pos2, vec2,
};

fn context(density: Density) -> Context {
    let ctx = Context::default();
    let mut fonts = FontDefinitions::default();
    crate::icons::install_fonts(&mut fonts);
    crate::fonts::install(&mut fonts);
    ctx.set_fonts(fonts);
    egui_extras::install_image_loaders(&ctx);
    metrics::set_density(&ctx, density);
    metrics::install_overlaid_style(&ctx, Style::default());
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
        events,
        ..Default::default()
    }
}

/// Runs `f` for `passes` frames and returns what its last pass produced.
fn run<R>(ctx: &Context, passes: usize, mut f: impl FnMut(&mut Ui) -> R) -> R {
    let mut out = None;
    for _ in 0..passes {
        let mut full = ctx.run_ui(input(Vec::new()), |ui| {
            CentralPanel::default().show(ui, |ui| out = Some(f(ui)));
        });
        full.textures_delta.clear();
    }
    out.expect("the ui closure ran")
}

fn run_with(ctx: &Context, events: Vec<Event>, mut f: impl FnMut(&mut Ui)) {
    let mut full = ctx.run_ui(input(events), |ui| {
        CentralPanel::default().show(ui, |ui| f(ui));
    });
    full.textures_delta.clear();
}

/// The first row's rect that leaves `height` or the shared centre line.
fn misaligned(rects: &[Rect], height: f32) -> Option<Rect> {
    let centre = rects.first()?.center().y;
    rects
        .iter()
        .copied()
        .find(|r| (r.height() - height).abs() > 0.01 || (r.center().y - centre).abs() > 0.01)
}

fn row(ui: &mut Ui) -> Vec<Rect> {
    let mut text = "Text".to_owned();
    let mut size = 20.0_f32;
    let mut choice = 0_usize;
    let mut on = true;
    ui.horizontal(|ui| {
        vec![
            button(ui, "Apply").rect,
            icon_button(ui, crate::icon_pack::GEAR, IconBox::Md, false, "Settings").rect,
            text_field(ui, &mut text, "", 120.0).rect,
            value_field(ui, &mut size, 1.0..=100.0, "px", 70.0).rect,
            dropdown(ui, "row", &mut choice, &[(0, "One"), (1, "Two")], 100.0).rect,
            toggle(ui, &mut on, "Live").rect,
        ]
    })
    .inner
}

#[test]
fn row_controls_share_control_height_and_centre_at_every_density() {
    for density in Density::ALL {
        let ctx = context(density);
        let rects = run(&ctx, 2, row);
        let height = Metrics::for_density(density).control_h;
        assert_eq!(rects.len(), 6);
        assert_eq!(misaligned(&rects, height), None, "{density:?}: {rects:?}");
    }
}

#[test]
fn the_alignment_check_catches_a_control_off_the_row() {
    let ctx = context(Density::Regular);
    let mut rects = run(&ctx, 2, row);
    let height = Metrics::for_density(Density::Regular).control_h;
    rects.push(rects[0].translate(vec2(0.0, 1.0)));
    assert!(misaligned(&rects, height).is_some());
    rects.pop();
    rects.push(Rect::from_min_size(rects[0].min, vec2(30.0, height + 2.0)));
    assert!(misaligned(&rects, height).is_some());
}

#[test]
fn prominent_buttons_take_the_button_height() {
    for density in Density::ALL {
        let ctx = context(density);
        let m = Metrics::for_density(density);
        let heights = run(&ctx, 1, |ui| {
            [
                primary_button(ui, "Save").rect.height(),
                secondary_button(ui, "Cancel").rect.height(),
                danger_button(ui, "Delete").rect.height(),
                ghost_button(ui, "More").rect.height(),
            ]
        });
        assert_eq!(heights, [m.button_h, m.button_h, m.button_h, m.control_h]);
    }
}

#[test]
fn icon_boxes_are_square_and_md_matches_the_control_height() {
    let ctx = context(Density::Compact);
    let m = Metrics::for_density(Density::Compact);
    let sizes = run(&ctx, 1, |ui| {
        [IconBox::Sm, IconBox::Md, IconBox::Lg, IconBox::Tool].map(|b| {
            icon_button(ui, crate::icon_pack::CPU, b, false, "")
                .rect
                .size()
        })
    });
    assert_eq!(sizes[1], Vec2::splat(m.control_h));
    for size in sizes {
        assert_eq!(size.x, size.y);
        assert_eq!(size.x, size.x.round());
    }
}

#[test]
fn bars_hold_their_height_whatever_they_contain() {
    let ctx = context(Density::Regular);
    let m = Metrics::for_density(Density::Regular);
    for kind in [
        BarKind::Tool,
        BarKind::Header,
        BarKind::Status,
        BarKind::Footer,
    ] {
        let (bar_rect, inner) = run(&ctx, 1, |ui| {
            let top = ui.cursor().top();
            let inner = bar(ui, kind, |ui| {
                icon_button(ui, crate::icon_pack::CPU, IconBox::Md, false, "").rect
            });
            (
                Rect::from_min_max(
                    pos2(0.0, top),
                    pos2(1.0, ui.cursor().top() - ui.spacing().item_spacing.y),
                ),
                inner,
            )
        });
        assert_eq!(bar_rect.height(), kind.height(&m), "{kind:?}");
        assert!(
            (inner.center().y - bar_rect.center().y).abs() < 0.01,
            "{kind:?} row is off centre"
        );
    }
}

#[test]
fn dialog_buttons_put_the_default_answer_first_and_cancel_last() {
    let buttons = [
        (Role::Cancel, "Cancel"),
        (Role::Secondary, "Don't save"),
        (Role::Primary, "Save"),
    ];
    assert_eq!(layout::dialog_order(&buttons), [2, 1, 0]);
    let danger = [(Role::Cancel, "Keep"), (Role::Danger, "Delete")];
    assert_eq!(layout::dialog_order(&danger), [1, 0]);
}

#[test]
fn a_toggle_flips_when_clicked() {
    let ctx = context(Density::Regular);
    let mut on = false;
    let mut at = Rect::NOTHING;
    run_with(&ctx, Vec::new(), |ui| at = toggle(ui, &mut on, "Live").rect);
    let p = at.center();
    let press = |pressed| Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    run_with(&ctx, vec![Event::PointerMoved(p), press(true)], |ui| {
        let _ = toggle(ui, &mut on, "Live");
    });
    run_with(&ctx, vec![press(false)], |ui| {
        let _ = toggle(ui, &mut on, "Live");
    });
    assert!(on);
}

#[test]
fn a_preview_state_is_scoped_to_its_closure() {
    let ctx = context(Density::Regular);
    run(&ctx, 1, |ui| {
        let response = button(ui, "x");
        assert_eq!(state_of(ui, &response), (State::Rest, false));
        with_preview(ui, Preview::Pressed, |ui| {
            assert_eq!(state_of(ui, &response), (State::Pressed, false));
            with_preview(ui, Preview::Focused, |ui| assert!(preview_focused(ui)));
            assert!(!preview_focused(ui));
        });
        assert_eq!(state_of(ui, &response), (State::Rest, false));
    });
}

#[test]
fn the_gallery_draws_at_every_density() {
    for density in Density::ALL {
        let ctx = context(density);
        run(&ctx, 2, gallery);
    }
}

#[test]
fn the_kit_draws_without_the_weight_families() {
    let ctx = Context::default();
    metrics::install_overlaid_style(&ctx, Style::default());
    run(&ctx, 1, gallery);
}

fn click_at(ctx: &Context, p: Pos2, mut f: impl FnMut(&mut Ui)) {
    let press = |pressed| Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    run_with(ctx, vec![Event::PointerMoved(p), press(true)], &mut f);
    run_with(ctx, vec![press(false)], &mut f);
}

#[test]
fn a_closed_section_stays_closed_until_clicked_and_a_forced_state_wins() {
    let ctx = context(Density::Regular);
    let mut header = Rect::NOTHING;
    let mut opened = false;
    run_with(&ctx, Vec::new(), |ui| {
        let top = ui.cursor().top();
        opened = Section::new("s", "Registers")
            .default_open(false)
            .header(ui);
        header = Rect::from_min_max(pos2(0.0, top), pos2(100.0, ui.cursor().top()));
    });
    assert!(!opened);
    click_at(&ctx, header.center(), |ui| {
        opened = Section::new("s", "Registers")
            .default_open(false)
            .header(ui);
    });
    run_with(&ctx, Vec::new(), |ui| {
        opened = Section::new("s", "Registers")
            .default_open(false)
            .header(ui);
    });
    assert!(opened, "a click opens it");
    run_with(&ctx, Vec::new(), |ui| {
        opened = Section::new("s", "Registers").open(Some(false)).header(ui);
    });
    assert!(!opened, "a forced state wins");
}

#[test]
fn a_section_stores_its_state_where_a_collapsing_header_would() {
    let ctx = context(Density::Regular);
    let id = run(&ctx, 1, |ui| {
        let _ = Section::new("salt", "T").default_open(false).header(ui);
        ui.make_persistent_id(eframe::egui::IdSalt::new("salt"))
    });
    let stored = eframe::egui::collapsing_header::CollapsingState::load(&ctx, id);
    assert!(stored.is_some_and(|s| !s.is_open()));
}

#[test]
fn a_segment_click_picks_its_value() {
    let ctx = context(Density::Regular);
    let mut value = 0_u8;
    let segments = [Segment::new(0_u8, "One"), Segment::new(1, "Two")];
    let mut rect = Rect::NOTHING;
    run_with(&ctx, Vec::new(), |ui| {
        rect = segmented(ui, &mut value, &segments).rect;
    });
    assert_eq!(
        rect.height(),
        Metrics::for_density(Density::Regular).control_h
    );
    let right = pos2(rect.right() - 4.0, rect.center().y);
    let mut changed = false;
    click_at(&ctx, right, |ui| {
        changed |= segmented(ui, &mut value, &segments).changed();
    });
    assert_eq!(value, 1);
    assert!(changed);
}

#[test]
fn a_slider_click_sets_the_value_under_the_pointer() {
    let ctx = context(Density::Regular);
    let mut value = 0.0_f32;
    let mut rect = Rect::NOTHING;
    run_with(&ctx, Vec::new(), |ui| {
        rect = Slider::new(&mut value, 0.0..=10.0)
            .width(200.0)
            .step(1.0)
            .show(ui)
            .rect;
    });
    click_at(&ctx, rect.right_center() - vec2(7.0, 0.0), |ui| {
        let _ = Slider::new(&mut value, 0.0..=10.0)
            .width(200.0)
            .step(1.0)
            .show(ui);
    });
    assert_eq!(value, 10.0);
}

#[test]
fn a_disabled_button_ignores_clicks() {
    let ctx = context(Density::Regular);
    let mut rect = Rect::NOTHING;
    let mut clicked = false;
    run_with(&ctx, Vec::new(), |ui| {
        rect = TextButton::new("Go").enabled(false).show(ui).rect;
    });
    click_at(&ctx, rect.center(), |ui| {
        clicked |= TextButton::new("Go").enabled(false).show(ui).clicked();
    });
    assert!(!clicked);
    run_with(&ctx, Vec::new(), |ui| {
        let _ = TextButton::new("Go").show(ui);
    });
    click_at(&ctx, rect.center(), |ui| {
        clicked |= TextButton::new("Go").show(ui).clicked();
    });
    assert!(clicked, "the enabled button at the same spot clicks");
}

#[test]
fn row_kit_additions_share_the_control_height() {
    for density in Density::ALL {
        let ctx = context(density);
        let m = Metrics::for_density(density);
        let heights = run(&ctx, 2, |ui| {
            let mut v = 1.0_f32;
            let mut seg = 0_u8;
            let mut pick = 9_u8;
            ui.horizontal(|ui| {
                [
                    ProgressBar::new(0.5).width(80.0).show(ui).rect.height(),
                    Slider::new(&mut v, 0.0..=2.0).show(ui).rect.height(),
                    segmented(ui, &mut seg, &[Segment::new(0, "A")])
                        .rect
                        .height(),
                    Pill::new("P")
                        .tone(Color32::RED)
                        .selected(true)
                        .show(ui)
                        .rect
                        .height(),
                    Dropdown::new("d", &mut pick, &[(0, "x")])
                        .placeholder("Pick")
                        .show(ui)
                        .rect
                        .height(),
                ]
            })
            .inner
        });
        assert!(
            heights.iter().all(|h| *h == m.control_h),
            "{density:?}: {heights:?}"
        );
    }
}

#[test]
fn a_text_area_grows_with_its_rows() {
    let ctx = context(Density::Regular);
    let (two, six) = run(&ctx, 1, |ui| {
        let mut a = String::new();
        let mut b = String::new();
        (
            TextArea::new(&mut a).rows(2).width(300.0).show(ui).rect,
            TextArea::new(&mut b).rows(6).width(300.0).show(ui).rect,
        )
    });
    assert_eq!(two.width(), 300.0);
    assert!(six.height() > two.height() + 3.0 * 12.0, "{two:?} {six:?}");
}

#[test]
fn a_value_field_formatter_shows_its_text() {
    let ctx = context(Density::Regular);
    let mut threads = 0_u32;
    let mut full = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = ValueField::new(&mut threads, 0..=64)
            .formatter(|v| {
                if v == 0.0 {
                    "auto".into()
                } else {
                    format!("{v}")
                }
            })
            .show(ui);
    });
    full.textures_delta.clear();
    let texts: Vec<String> = full
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            eframe::egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t == "auto"), "{texts:?}");
}

#[test]
fn an_icon_window_keeps_the_plain_title_id() {
    let ctx = context(Density::Regular);
    let mut full = ctx.run_ui(input(Vec::new()), |ui| {
        icon_window(ui.ctx(), crate::icon_pack::GEAR, "Settings").show(ui.ctx(), |_| {});
        eframe::egui::Window::new("Plain").show(ui.ctx(), |_| {});
    });
    full.textures_delta.clear();
    let known = |id| ctx.memory(|m| m.area_rect(id).is_some());
    assert!(known(window_id("Plain")), "window_id matches Window::new");
    assert!(
        known(window_id("Settings")),
        "the icon window keeps the plain id"
    );
}

#[test]
fn a_tree_node_toggles_from_its_caret() {
    let ctx = context(Density::Regular);
    let mut row = Rect::NOTHING;
    let mut shown = false;
    let draw = |ui: &mut Ui, row: &mut Rect, shown: &mut bool| {
        let (r, inner) = ListItem::new("C:").show_node(ui, "c", false, |_| ());
        *row = r.rect;
        *shown = inner.is_some();
    };
    run_with(&ctx, Vec::new(), |ui| draw(ui, &mut row, &mut shown));
    assert!(!shown);
    let caret = row.left_center() + vec2(12.0, 0.0);
    click_at(&ctx, caret, |ui| draw(ui, &mut row, &mut shown));
    run_with(&ctx, Vec::new(), |ui| draw(ui, &mut row, &mut shown));
    assert!(shown, "a caret click opens the node");
}

#[test]
fn one_tab_press_focuses_the_text_inside_a_field() {
    let ctx = context(Density::Regular);
    let mut text = String::new();
    let mut draw = |ui: &mut Ui| {
        let _ = TextField::new(&mut text).id_salt("tab.field").show(ui);
    };
    run_with(&ctx, Vec::new(), &mut draw);
    let tab = Event::Key {
        key: eframe::egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    run_with(&ctx, vec![tab], &mut draw);
    run_with(&ctx, Vec::new(), &mut draw);
    let focused = ctx.memory(|m| m.has_focus(eframe::egui::Id::unique("tab.field")));
    assert!(focused, "the first Tab lands on the text, not the box");
}

#[test]
fn disabled_menu_rows_and_dialog_buttons_ignore_clicks() {
    let ctx = context(Density::Regular);
    let mut rects = (Rect::NOTHING, Rect::NOTHING);
    let mut hits = (false, None);
    let draw = |ui: &mut Ui, rects: &mut (Rect, Rect), hits: &mut (bool, Option<usize>)| {
        let row = MenuItem::new("Archive").enabled(false).show(ui);
        rects.0 = row.rect;
        hits.0 |= row.clicked();
        let buttons = [(Role::Primary, "Apply")];
        let top = ui.cursor().top();
        hits.1 = hits.1.or(dialog_buttons_enabled(ui, &buttons, &[false]));
        rects.1 = Rect::from_min_max(
            pos2(ui.max_rect().right() - 40.0, top),
            pos2(ui.max_rect().right() - 20.0, top + 20.0),
        );
    };
    run_with(&ctx, Vec::new(), |ui| draw(ui, &mut rects, &mut hits));
    let (row, button) = rects;
    click_at(&ctx, row.center(), |ui| draw(ui, &mut rects, &mut hits));
    click_at(&ctx, button.center(), |ui| draw(ui, &mut rects, &mut hits));
    assert_eq!(hits, (false, None));
}

#[test]
fn a_section_reports_its_header_and_takes_an_exact_id() {
    let ctx = context(Density::Regular);
    let id = eframe::egui::Id::unique("exact.section");
    let mut header = Rect::NOTHING;
    let mut clicked = false;
    let draw = |ui: &mut Ui, header: &mut Rect, clicked: &mut bool| {
        let (response, _) = Section::new("unused", "Inventory")
            .id(id)
            .suffix("(3)", None)
            .trailing(|ui| {
                let _ = badge(ui, "1 running", Color32::YELLOW);
            })
            .default_open(false)
            .show_with_header(ui, |_| ());
        *header = response.rect;
        *clicked |= response.clicked();
    };
    run_with(&ctx, Vec::new(), |ui| draw(ui, &mut header, &mut clicked));
    assert_eq!(
        header.height(),
        Metrics::for_density(Density::Regular).header_h
    );
    click_at(&ctx, header.left_center() + vec2(30.0, 0.0), |ui| {
        draw(ui, &mut header, &mut clicked)
    });
    assert!(clicked);
    let stored = eframe::egui::collapsing_header::CollapsingState::load(&ctx, id);
    assert!(
        stored.is_some_and(|s| s.is_open()),
        "the click opened the exact id"
    );
}

#[test]
fn a_loading_row_sits_on_one_line() {
    let ctx = context(Density::Regular);
    let (row, line) = run(&ctx, 1, |ui| {
        (
            loading_row(ui, "Loading drivers").rect,
            ui.text_style_height(&eframe::egui::TextStyle::Body),
        )
    });
    assert!(
        (row.height() - line.max(SpinnerSize::Sm.side())).abs() < 0.5,
        "{row:?} {line}"
    );
    assert_eq!(row.width(), row.width().round());
}

#[test]
fn a_menu_without_a_set_width_fits_its_rows() {
    let ctx = context(Density::Regular);
    let trigger = std::cell::Cell::new(Rect::NOTHING);
    let mut rows = Vec::new();
    let mut draw = |ui: &mut Ui| {
        let (response, _) = MenuButton::new("View").show(ui, |ui| {
            rows.clear();
            rows.push(
                menu_item(ui, crate::icon_pack::GEAR, "Settings")
                    .rect
                    .width(),
            );
            menu_separator(ui);
            rows.push(menu_check_item(ui, None, "Wrap lines", true).rect.width());
        });
        trigger.set(response.rect);
    };
    run_with(&ctx, Vec::new(), &mut draw);
    click_at(&ctx, trigger.get().center(), &mut draw);
    for _ in 0..3 {
        run_with(&ctx, Vec::new(), &mut draw);
    }
    assert_eq!(rows.len(), 2, "the menu opened");
    for width in rows {
        assert!(
            (width - layout::MENU_MIN_W).abs() < 1.0,
            "menu row is {width} wide"
        );
    }
}

#[test]
fn a_checkbox_row_flips_from_its_label_and_holds_the_control_height() {
    let ctx = context(Density::Comfortable);
    let mut on = false;
    let mut row = Rect::NOTHING;
    run_with(&ctx, Vec::new(), |ui| {
        row = checkbox(ui, &mut on, "Startup.ps1").rect
    });
    assert_eq!(
        row.height(),
        Metrics::for_density(Density::Comfortable).control_h
    );
    click_at(&ctx, row.right_center() - vec2(4.0, 0.0), |ui| {
        let _ = checkbox(ui, &mut on, "Startup.ps1");
    });
    assert!(on);
}

#[test]
fn an_icon_link_reports_the_width_it_allocates() {
    let ctx = context(Density::Regular);
    for (text, small) in [("Link", true), ("customer:abc123", false)] {
        let (reported, drawn) = run(&ctx, 2, |ui| {
            let link = IconLink::new(text).icon(crate::icon_pack::LINK).small(small);
            (link.width(ui), link.show(ui).rect.width())
        });
        assert_eq!(reported, drawn, "{text}");
    }
}
