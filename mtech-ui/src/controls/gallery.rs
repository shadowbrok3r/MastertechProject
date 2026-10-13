//! Every kit control in every state, for the theme editor's live preview.

use eframe::egui::{Align, Frame, Id, Layout, Ui, Vec2};

use super::{
    BarKind, ButtonKind, Dropdown, IconBox, IconButton, IconLabel, IconLink, ListItem, MenuButton,
    MenuItem, Pill, Preview, ProgressBar, Role, Section, Segment, Slider, SpinnerSize, TextArea,
    TextButton, TextField, Tokens, ValueField, badge, bar, button, callout, card, checkbox,
    count_badge, dialog_buttons_enabled, dialog_contents, dropdown, elided_label, empty_state,
    ghost_button, heading, icon_button, icon_hyperlink, icon_label, icon_link, icon_text_button,
    kit, loading_row, loading_state, menu_body, menu_check_item, menu_danger_item, menu_item,
    menu_picker, menu_separator, meter, panel_header, pill, search_field, section_header,
    section_label, segmented, spinner, status_label, subheading, submenu, text_field,
    tinted_spinner, toggle, value_field, vline, with_preview,
};
use crate::icon_pack::{self as svg, Icon};

/// Width of the gallery's text fields.
const FIELD_W: f32 = 160.0;
/// Width of the gallery's numeric fields.
const VALUE_W: f32 = 76.0;
/// Width of the gallery's dropdowns.
const DROPDOWN_W: f32 = 120.0;
/// Width of the gallery's progress bars and meters.
const BAR_W: f32 = 220.0;
/// Width of the gallery's list.
const LIST_W: f32 = 280.0;
/// Height of the gallery's vertical rules.
const VLINE_H: f32 = 18.0;
/// Size of a sample in the icon strip.
const ICON_SAMPLE: f32 = 20.0;
/// Widest the sample dialog gets.
const DIALOG_W: f32 = 420.0;
/// Inner margin of the sample dialog.
const DIALOG_MARGIN: i8 = 16;

/// An answer on a checklist row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Pass,
    Fail,
    Skip,
}

/// Values the gallery's live controls edit.
#[derive(Clone, Debug)]
struct Samples {
    text: String,
    search: String,
    service: String,
    notes: String,
    size: f32,
    percent: u8,
    threads: u32,
    choice: usize,
    unset: usize,
    on: bool,
    off: bool,
    pill: usize,
    answer: Option<Answer>,
    view: usize,
    scale: f32,
    picked: usize,
}

impl Default for Samples {
    fn default() -> Self {
        Self {
            text: "Laptop intake".to_owned(),
            search: String::new(),
            service: "21549".to_owned(),
            notes: "Customer reports random shutdowns under load.\nFans clean.".to_owned(),
            size: 20.0,
            percent: 85,
            threads: 0,
            choice: 1,
            unset: usize::MAX,
            on: true,
            off: false,
            pill: 0,
            answer: Some(Answer::Pass),
            view: 0,
            scale: 1.5,
            picked: 0,
        }
    }
}

const CHOICES: [(usize, &str); 4] = [
    (0, "Compact"),
    (1, "Regular"),
    (2, "Comfortable"),
    (3, "A very long option label"),
];

const STRIP: [Icon; 18] = [
    svg::CPU,
    svg::MEMORY,
    svg::HARD_DRIVE,
    svg::GRAPHICS_CARD,
    svg::THERMOMETER,
    svg::FAN,
    svg::BATTERY_CHARGING,
    svg::WIFI_HIGH,
    svg::USB,
    svg::MONITOR,
    svg::KEYBOARD,
    svg::PRINTER,
    svg::STOREFRONT,
    svg::RECEIPT,
    svg::PACKAGE,
    svg::TRUCK,
    svg::SHIELD_CHECK,
    svg::ROBOT,
];

/// Every kit control in collapsible sections, in rest, hover, press, focus and disabled states.
pub fn gallery(ui: &mut Ui) {
    let id = Id::new("mtech.controls.gallery");
    let mut s = ui.data(|d| d.get_temp::<Samples>(id)).unwrap_or_default();
    ui.push_id(id, |ui| {
        buttons(ui);
        icon_buttons(ui);
        fields(ui, &mut s);
        alignment(ui, &mut s);
        meters(ui, &mut s);
        labels(ui, &mut s);
        sections(ui);
        surfaces(ui);
        lists(ui);
        bars(ui, &mut s);
        empty(ui);
        dialog(ui);
        menus(ui, &mut s);
        icons(ui);
    });
    ui.data_mut(|d| d.insert_temp(id, s));
}

/// `add` drawn once per state: rest, hovered, pressed, focused, disabled.
fn states(ui: &mut Ui, mut add: impl FnMut(&mut Ui, &str)) {
    ui.horizontal(|ui| {
        add(ui, "Rest");
        with_preview(ui, Preview::Hovered, |ui| add(ui, "Hover"));
        with_preview(ui, Preview::Pressed, |ui| add(ui, "Pressed"));
        with_preview(ui, Preview::Focused, |ui| add(ui, "Focus"));
        ui.add_enabled_ui(false, |ui| add(ui, "Disabled"));
    });
}

fn buttons(ui: &mut Ui) {
    if !section_header(ui, "gallery.buttons", svg::CURSOR_CLICK, "Buttons") {
        return;
    }
    for kind in [
        ButtonKind::Neutral,
        ButtonKind::Primary,
        ButtonKind::Danger,
        ButtonKind::Ghost,
    ] {
        let prominent = matches!(kind, ButtonKind::Primary | ButtonKind::Danger);
        states(ui, |ui, label| {
            let _ = TextButton::new(label)
                .kind(kind)
                .prominent(prominent)
                .show(ui);
        });
    }
    states(ui, |ui, label| {
        let _ = icon_text_button(ui, svg::FLOPPY_DISK, label);
    });
    ui.horizontal(|ui| {
        let t = Tokens::get(ui.ctx());
        let _ = TextButton::new("WRSA-7F2K")
            .icon(svg::KEY)
            .ink(t.warn)
            .mono(true)
            .show(ui);
        let _ = TextButton::new("SO 1000001")
            .kind(ButtonKind::Ghost)
            .ink(t.success)
            .show(ui);
        let _ = TextButton::new("Disabled")
            .icon(svg::TRASH)
            .enabled(false)
            .show(ui);
        let _ = IconButton::new(svg::GEAR, IconBox::Md)
            .tip("Disabled")
            .enabled(false)
            .show(ui);
    });
}

fn icon_buttons(ui: &mut Ui) {
    if !section_header(
        ui,
        "gallery.icon_buttons",
        svg::SQUARES_FOUR,
        "Icon buttons",
    ) {
        return;
    }
    ui.horizontal(|ui| {
        for size in [IconBox::Sm, IconBox::Md, IconBox::Lg, IconBox::Tool] {
            let _ = icon_button(ui, svg::GEAR, size, false, "Settings");
        }
        vline(ui, VLINE_H);
        for size in [IconBox::Sm, IconBox::Md, IconBox::Lg, IconBox::Tool] {
            let _ = icon_button(ui, svg::MT_STRESS, size, true, "Stress test");
        }
    });
    states(ui, |ui, label| {
        let _ = icon_button(ui, svg::PUSH_PIN, IconBox::Md, false, label);
        let _ = icon_button(ui, svg::PUSH_PIN, IconBox::Md, true, label);
        let t = Tokens::get(ui.ctx());
        let _ = IconButton::new(svg::LINK_BREAK, IconBox::Md)
            .tint(t.error)
            .tip(label)
            .show(ui);
    });
    ui.horizontal(|ui| {
        let t = Tokens::get(ui.ctx());
        for (icon, selected, count, tone) in [
            (svg::BELL_RINGING, false, 3, t.error),
            (svg::MOON, true, 12, t.secondary),
            (svg::CHAT_TEXT, false, 120, t.primary),
        ] {
            let response = icon_button(ui, icon, IconBox::Md, selected, icon.name());
            count_badge(ui, response.rect, count, tone);
        }
    });
}

fn fields(ui: &mut Ui, s: &mut Samples) {
    if !section_header(ui, "gallery.fields", svg::TEXT_AA, "Fields") {
        return;
    }
    ui.horizontal(|ui| {
        let mut empty = String::new();
        let _ = text_field(ui, &mut empty, "Customer name", FIELD_W);
        let _ = text_field(ui, &mut s.text, "", FIELD_W);
        let _ = search_field(ui, &mut s.search, "Search tasks", FIELD_W);
    });
    ui.horizontal(|ui| {
        let mut focused = "Focused".to_owned();
        with_preview(ui, Preview::Focused, |ui| {
            text_field(ui, &mut focused, "", FIELD_W)
        });
        let mut hovered = "Hovered".to_owned();
        with_preview(ui, Preview::Hovered, |ui| {
            text_field(ui, &mut hovered, "", FIELD_W)
        });
        let mut disabled = "Disabled".to_owned();
        let _ = TextField::new(&mut disabled)
            .width(FIELD_W)
            .enabled(false)
            .show(ui);
        let _ = Dropdown::new("gallery.disabled", &mut s.choice, &CHOICES)
            .width(DROPDOWN_W)
            .enabled(false)
            .show(ui);
    });
    ui.horizontal(|ui| {
        let invalid = s.service.len() != 7;
        let _ = TextField::new(&mut s.service)
            .hint("Service number")
            .width(FIELD_W)
            .icon(svg::RECEIPT)
            .mono(true)
            .char_limit(7)
            .invalid(invalid)
            .show(ui);
        let _ = ValueField::new(&mut s.threads, 0..=64)
            .width(VALUE_W)
            .suffix("thr")
            .formatter(|v| {
                if v == 0.0 {
                    "auto".to_owned()
                } else {
                    format!("{v:.0}")
                }
            })
            .parser(|text| {
                if text.trim().eq_ignore_ascii_case("auto") {
                    Some(0.0)
                } else {
                    text.trim().parse().ok()
                }
            })
            .show(ui);
        let _ = Dropdown::new("gallery.unset", &mut s.unset, &CHOICES)
            .width(DROPDOWN_W)
            .placeholder("Pick a density")
            .show(ui);
    });
    ui.horizontal(|ui| {
        let _ = value_field(ui, &mut s.size, 1.0..=2500.0, "px", VALUE_W);
        let _ = value_field(ui, &mut s.percent, 0..=100, "%", VALUE_W);
        with_preview(ui, Preview::Focused, |ui| {
            value_field(ui, &mut s.size, 1.0..=2500.0, "px", VALUE_W)
        });
        let _ = dropdown(ui, "gallery.density", &mut s.choice, &CHOICES, DROPDOWN_W);
        with_preview(ui, Preview::Hovered, |ui| {
            dropdown(ui, "gallery.hover", &mut s.choice, &CHOICES, DROPDOWN_W)
        });
    });
    ui.horizontal(|ui| {
        let _ = toggle(ui, &mut s.on, "Backdrop blur");
        let _ = toggle(ui, &mut s.off, "Auto-assign");
        with_preview(ui, Preview::Focused, |ui| toggle(ui, &mut s.off, "Focus"));
        ui.add_enabled_ui(false, |ui| toggle(ui, &mut s.on, "Disabled"));
    });
    ui.horizontal(|ui| {
        let _ = checkbox(ui, &mut s.on, "Startup.ps1");
        let _ = checkbox(ui, &mut s.off, "Cleanup.ps1");
        with_preview(ui, Preview::Hovered, |ui| {
            checkbox(ui, &mut s.off, "Hovered")
        });
        with_preview(ui, Preview::Focused, |ui| checkbox(ui, &mut s.on, "Focus"));
        ui.add_enabled_ui(false, |ui| checkbox(ui, &mut s.on, "Disabled"));
    });
    let _ = TextArea::new(&mut s.notes)
        .hint("Check-in notes")
        .rows(3)
        .show(ui);
    let mut log = "[09:14:02] stress: lane 0 started\n[09:14:03] stress: lane 1 started";
    let _ = TextArea::new(&mut log).mono(true).rows(2).show(ui);
}

/// One row holding every row control.
fn alignment(ui: &mut Ui, s: &mut Samples) {
    if !section_header(ui, "gallery.alignment", svg::ROWS, "Row alignment") {
        return;
    }
    ui.horizontal(|ui| {
        let _ = button(ui, "Apply");
        let _ = icon_button(ui, svg::ARROW_CLOCKWISE, IconBox::Md, false, "Refresh");
        let _ = TextField::new(&mut s.text)
            .width(FIELD_W)
            .icon(svg::USER)
            .show(ui);
        let _ = value_field(ui, &mut s.size, 1.0..=2500.0, "px", VALUE_W);
        let _ = dropdown(ui, "gallery.align", &mut s.choice, &CHOICES, DROPDOWN_W);
        let _ = toggle(ui, &mut s.on, "Live");
    });
}

fn meters(ui: &mut Ui, s: &mut Samples) {
    if !section_header(
        ui,
        "gallery.meters",
        svg::GAUGE,
        "Progress, sliders and choices",
    ) {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let _ = ProgressBar::new(0.62)
            .width(BAR_W)
            .text("stage 3/8 · fp")
            .show(ui);
        let _ = ProgressBar::new(0.35)
            .width(BAR_W)
            .percentage(true)
            .show(ui);
        let _ = ProgressBar::indeterminate().width(BAR_W / 2.0).show(ui);
    });
    ui.horizontal(|ui| {
        for (share, tone) in [
            (0.9, t.success),
            (0.55, t.warn),
            (0.2, t.error),
            (0.7, t.info),
        ] {
            let _ = meter(ui, share, BAR_W / 2.0, tone);
        }
    });
    ui.horizontal(|ui| {
        let _ = Slider::new(&mut s.scale, 0.1..=10.0).value("x").show(ui);
        with_preview(ui, Preview::Hovered, |ui| {
            Slider::new(&mut s.scale, 0.1..=10.0).show(ui)
        });
        ui.add_enabled_ui(false, |ui| Slider::new(&mut s.scale, 0.1..=10.0).show(ui));
    });
    ui.horizontal(|ui| {
        let _ = segmented(
            ui,
            &mut s.answer,
            &[
                Segment::new(Some(Answer::Pass), "Pass")
                    .icon(svg::CHECK_CIRCLE)
                    .tone(t.success),
                Segment::new(Some(Answer::Fail), "Fail")
                    .icon(svg::X_CIRCLE)
                    .tone(t.error),
                Segment::new(Some(Answer::Skip), "N/A").icon(svg::MINUS_CIRCLE),
            ],
        );
        let _ = segmented(
            ui,
            &mut s.view,
            &[
                Segment::new(0, "List").icon(svg::LIST),
                Segment::new(1, "Grid").icon(svg::SQUARES_FOUR),
                Segment::new(2, "Chart").icon(svg::CHART_LINE),
            ],
        );
    });
    ui.horizontal(|ui| {
        let _ = Pill::new("Passed")
            .icon(svg::CHECK_CIRCLE)
            .selected(true)
            .tone(t.success)
            .show(ui);
        let _ = Pill::new("Failed")
            .icon(svg::X_CIRCLE)
            .selected(true)
            .tone(t.error)
            .show(ui);
        let _ = Pill::new("Skipped").icon(svg::MINUS_CIRCLE).show(ui);
    });
}

fn labels(ui: &mut Ui, s: &mut Samples) {
    if !section_header(ui, "gallery.labels", svg::TAG, "Labels and chips") {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let _ = heading(ui, svg::MT_STRESS, "Stress test");
    let _ = subheading(ui, svg::CPU, "Processor");
    section_label(ui, "Section label");
    ui.horizontal(|ui| {
        let _ = icon_label(ui, svg::DESKTOP_TOWER, "DESKTOP-3C0R0D7");
        let _ = elided_label(
            ui,
            "A label far too long for the room it is given in this row",
            180.0,
        );
    });
    ui.horizontal(|ui| {
        let _ = status_label(ui, svg::CHECK_CIRCLE, "Drivers current", t.success);
        let _ = status_label(ui, svg::WARNING, "BIOS 1.A5 is old", t.warn);
        let _ = status_label(ui, svg::X_CIRCLE, "WHEA 19 logged", t.error);
        let _ = IconLabel::new(svg::CLOCK, "2 minutes ago")
            .small(true)
            .tone(t.text_dim)
            .show(ui);
    });
    ui.horizontal(|ui| {
        let _ = IconLabel::new(svg::CPU, "Ryzen 9 7950X")
            .strong(true)
            .show(ui);
        let _ = IconLabel::new(svg::PLUGS_CONNECTED, "relay://DESKTOP-3C0R0D7")
            .mono(true)
            .show(ui);
        let _ = loading_row(ui, "Loading drivers");
    });
    ui.allocate_ui(Vec2::new(LIST_W, 0.0), |ui| {
        let _ = IconLabel::new(
            svg::WARNING_CIRCLE,
            "relay closed the socket before the completion frame arrived",
        )
        .tone(t.error)
        .truncate(true)
        .show(ui);
    });
    ui.horizontal(|ui| {
        let _ = icon_link(ui, svg::RECEIPT, "Order 1000001");
        let _ = icon_hyperlink(
            ui,
            svg::ARROW_SQUARE_OUT,
            "Update Catalog",
            "https://www.catalog.update.microsoft.com",
        );
        with_preview(ui, Preview::Hovered, |ui| {
            icon_link(ui, svg::USER, "Hovered link")
        });
        let _ = IconLink::new("Small link")
            .icon(svg::LINK)
            .small(true)
            .show(ui);
    });
    let _ = IconLabel::new(
        svg::INFO,
        "A wrapped, selectable note that runs on past the end of the row so it breaks onto a \
         second line beside its icon.",
    )
    .wrap(true)
    .selectable(true)
    .tone(t.text_dim)
    .show(ui);
    ui.horizontal(|ui| {
        let _ = badge(ui, "Passed", t.success);
        let _ = badge(ui, "3 warnings", t.warn);
        let _ = badge(ui, "Failed", t.error);
        let _ = badge(ui, "Relay", t.info);
        let _ = badge(ui, "12", t.primary);
        let _ = badge(ui, "Draft", t.secondary);
    });
    ui.horizontal(|ui| {
        for (i, (icon, label)) in [
            (svg::LIST_CHECKS, "Open"),
            (svg::CLOCK, "Waiting"),
            (svg::CHECK_CIRCLE, "Done"),
        ]
        .into_iter()
        .enumerate()
        {
            if pill(ui, icon, label, s.pill == i).clicked() {
                s.pill = i;
            }
        }
        with_preview(ui, Preview::Hovered, |ui| pill(ui, None, "Hover", false));
        let _ = Pill::new("Threads")
            .icon(svg::CHAT_TEXT)
            .trailing(svg::CARET_DOWN)
            .show(ui);
    });
    ui.horizontal(|ui| {
        let _ = spinner(ui, SpinnerSize::Sm);
        let _ = spinner(ui, SpinnerSize::Md);
        let _ = spinner(ui, SpinnerSize::Lg);
        vline(ui, VLINE_H);
        let _ = tinted_spinner(ui, SpinnerSize::Md, t.warn);
        let _ = tinted_spinner(ui, SpinnerSize::Md, t.info);
    });
}

fn sections(ui: &mut Ui) {
    if !section_header(ui, "gallery.sections", svg::ROWS, "Sections") {
        return;
    }
    let _ = Section::new("gallery.section.open", "Loaded drivers")
        .icon(svg::CUBE)
        .show(ui, |ui| {
            let _ = icon_label(ui, svg::CUBE, "nvlddmkm.sys");
        });
    let _ = Section::new("gallery.section.closed", "Registers")
        .icon(svg::BINARY)
        .default_open(false)
        .show(ui, |ui| {
            ui.label("rax 0000000000000000");
        });
    let t = Tokens::get(ui.ctx());
    let _ = Section::new("gallery.section.rich", "Logan")
        .icon(svg::USER_CIRCLE)
        .suffix("(you)", None)
        .suffix("4", None)
        .suffix("\u{00b7} 2 working", t.info)
        .default_open(false)
        .header(ui);
    let _ = Section::new("gallery.section.trailing", "Hardware inventory")
        .icon(svg::CIRCUITRY)
        .icon_tone(t.secondary)
        .title_tone(t.secondary)
        .suffix("(6 components)", None)
        .trailing(|ui| {
            let _ = IconLabel::new(svg::MT_STRESS, "1 stress run in progress")
                .tone(t.warn)
                .show(ui);
        })
        .default_open(false)
        .header(ui);
}

fn surfaces(ui: &mut Ui) {
    if !section_header(ui, "gallery.surfaces", svg::STACK, "Cards and callouts") {
        return;
    }
    let t = Tokens::get(ui.ctx());
    panel_header(ui, svg::PACKAGE, "Store inventory", |ui| {
        let _ = icon_button(ui, svg::ARROW_CLOCKWISE, IconBox::Md, false, "Refresh");
    });
    card(ui, |ui| {
        let _ = subheading(ui, svg::HARD_DRIVE, "Samsung 980 PRO 1TB");
        let _ = status_label(ui, svg::CHECK_CIRCLE, "SMART healthy", t.success);
    });
    callout(ui, t.warn, svg::WARNING, |ui| {
        ui.label("Webroot is still scanning. Leave it running and keep working.");
    });
    callout(ui, t.info, None, |ui| {
        ui.label("A callout without an icon.");
    });
    let _ = loading_state(ui, "Loading store inventory");
}

fn lists(ui: &mut Ui) {
    if !section_header(ui, "gallery.lists", svg::LIST, "Lists and trees") {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.allocate_ui(Vec2::new(LIST_W, 0.0), |ui| {
        let _ = ListItem::new("Desktop").icon(svg::DESKTOP).show(ui);
        let _ = ListItem::new("Downloads")
            .icon(svg::DOWNLOAD)
            .selected(true)
            .show(ui);
        with_preview(ui, Preview::Hovered, |ui| {
            ListItem::new("Documents").icon(svg::FOLDER).show(ui)
        });
        let _ =
            ListItem::new("C:")
                .icon(svg::HARD_DRIVE)
                .show_node(ui, "gallery.tree.c", true, |ui| {
                    let _ = ListItem::new("Windows")
                        .icon(svg::FOLDER)
                        .detail("24.1 GB")
                        .show_node(ui, "gallery.tree.windows", false, |_| {});
                    let _ = ListItem::new("pagefile.sys")
                        .icon(svg::FILE)
                        .icon_tone(t.text_faint)
                        .detail("16 GB")
                        .reserve_caret(true)
                        .show(ui);
                });
    });
}

fn bars(ui: &mut Ui, s: &mut Samples) {
    if !section_header(ui, "gallery.bars", svg::LAYOUT, "Bars") {
        return;
    }
    bar(ui, BarKind::Tool, |ui| {
        let _ = icon_button(ui, svg::MT_LOGO, IconBox::Lg, true, "MasterTech");
        vline(ui, VLINE_H);
        let _ = icon_button(ui, svg::PLAY, IconBox::Md, false, "Run");
        let _ = icon_button(ui, svg::PAUSE, IconBox::Md, false, "Pause");
        let _ = icon_button(ui, svg::STOP, IconBox::Md, false, "Stop");
        vline(ui, VLINE_H);
        let _ = search_field(ui, &mut s.search, "Filter", FIELD_W);
        let _ = ghost_button(ui, "Clear");
    });
    bar(ui, BarKind::Status, |ui| {
        let t = Tokens::get(ui.ctx());
        let _ = icon_label(ui, svg::PLUGS_CONNECTED, "Connected");
        let _ = badge(ui, "Relay", t.info);
        let _ = elided_label(ui, "Last sync 2 minutes ago", 200.0);
    });
    bar(ui, BarKind::Footer, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let buttons = [(Role::Cancel, "Cancel"), (Role::Primary, "Save")];
            dialog_buttons_enabled(ui, &buttons, &[true, false]);
        });
    });
}

fn empty(ui: &mut Ui) {
    if !section_header(ui, "gallery.empty", svg::TRAY, "Empty state") {
        return;
    }
    empty_state(
        ui,
        svg::MT_INTAKE,
        "No machines waiting",
        "Machines checked in at the counter show up here.",
        &[(Role::Primary, "Check in"), (Role::Secondary, "Import")],
    );
}

fn dialog(ui: &mut Ui) {
    if !section_header(ui, "gallery.dialog", svg::APP_WINDOW, "Dialog") {
        return;
    }
    let width = ui.available_width().min(DIALOG_W) - 2.0 * DIALOG_MARGIN as f32;
    let buttons = [(Role::Cancel, "Keep"), (Role::Danger, "Wipe drive")];
    let title = "Wipe the system drive?";
    let body = "Every partition on Disk 0 is erased. Customer files cannot be recovered.";
    Frame::window(ui.style())
        .inner_margin(DIALOG_MARGIN)
        .show(ui, |ui| {
            dialog_contents(ui, Some(svg::HARD_DRIVE), title, width, &buttons, |ui| {
                ui.label(body);
            });
        });
}

fn menus(ui: &mut Ui, s: &mut Samples) {
    if !section_header(ui, "gallery.menus", svg::LIST, "Menus") {
        return;
    }
    let (t, _) = kit(ui);
    states(ui, |ui, label| {
        let _ = MenuButton::new(label).icon(svg::TABS).show(ui, |ui| {
            let _ = menu_item(ui, svg::GEAR, "Settings");
        });
    });
    ui.horizontal(|ui| {
        let _ = MenuButton::new("Tune-up / QC")
            .icon(svg::WRENCH)
            .tint(t.primary)
            .show(ui, |ui| {
                let _ = menu_check_item(ui, svg::WRENCH, "Tune-up / QC", true);
            });
        let _ = MenuButton::new("")
            .icon(svg::LAYOUT)
            .tip("Layout")
            .show(ui, |ui| {
                let _ = menu_item(ui, svg::FLOPPY_DISK, "Save layout");
            });
        let _ = menu_picker(
            ui,
            svg::STOREFRONT,
            "Store",
            &mut s.picked,
            &[(0, "Orem"), (1, "Provo"), (2, "Warehouse")],
        );
    });
    Frame::menu(ui.style()).show(ui, |ui| {
        menu_body(ui, |ui| {
            let _ = menu_check_item(ui, svg::LIST_CHECKS, "My Tasks", true);
            let _ = menu_check_item(ui, svg::STOREFRONT, "Store Tasks", false);
            menu_separator(ui);
            let _ = menu_item(ui, svg::MT_REMOTE, "Open remote session");
            with_preview(ui, Preview::Hovered, |ui| {
                menu_item(ui, svg::MT_STRESS, "Run stress test")
            });
            let _ = submenu(ui, svg::MT_FIRMWARE, "Firmware", |ui| {
                let _ = menu_item(ui, svg::DOWNLOAD, "Download BIOS");
            });
            menu_separator(ui);
            let _ = menu_item(ui, None, "Properties");
            let _ = MenuItem::new("Archive 0 idle")
                .icon(svg::ARCHIVE)
                .enabled(false)
                .show(ui);
            let _ = MenuItem::new("Batch actions")
                .icon(svg::STACK)
                .tone(t.warn)
                .submenu(ui, |ui| {
                    let _ = menu_item(ui, svg::ARROWS_CLOCKWISE, "Restart all");
                });
            let _ = menu_danger_item(ui, svg::TRASH, "Delete file");
            with_preview(ui, Preview::Hovered, |ui| {
                menu_danger_item(ui, svg::TRASH, "Delete folder")
            });
        });
    });
}

fn icons(ui: &mut Ui) {
    if !section_header(ui, "gallery.icons", svg::MT_LOGO, "Icons") {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        for icon in svg::ALL.iter().filter(|i| i.name().starts_with("mt-")) {
            let (rect, response) = ui
                .allocate_exact_size(Vec2::splat(ICON_SAMPLE + 8.0), eframe::egui::Sense::hover());
            icon.paint_duo(
                ui,
                rect,
                ICON_SAMPLE,
                t.text,
                t.primary.gamma_multiply(0.45),
            );
            response.on_hover_text(icon.name());
        }
    });
    ui.horizontal_wrapped(|ui| {
        for icon in STRIP {
            let (rect, response) = ui
                .allocate_exact_size(Vec2::splat(ICON_SAMPLE + 8.0), eframe::egui::Sense::hover());
            icon.paint(ui, rect, ICON_SAMPLE, t.icon);
            response.on_hover_text(icon.name());
        }
    });
}
