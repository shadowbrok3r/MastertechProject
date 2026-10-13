//! Task modal geometry: the inset pages' cards share their edges, sit centred, and hold their contents.

use database::schema::{
    CustomerData, LiveTaskPayload, RecordId, Status, TaskHistory, TaskHistoryLoad, TicketData, User,
};
use eframe::egui::{
    Color32, Context, Pos2, RawInput, Rect, Shape, Stroke, TextureId, Ui, UiBuilder,
    epaint::{ClippedPrimitive, ClippedShape},
    pos2, vec2,
};

use super::{ModalAction, TaskModal};
use crate::chats::ChatView;
use crate::modals::tabs::display_ticket_page;
use crate::modals::{ModalType, ModalWindow};
use crate::ui_tools::{
    glass_card, headless_render::Headless, metrics::Density, theme_config::PresetStyles,
};

/// Headless screen height.
const SCREEN_H: f32 = 1000.0;
/// Screen widths the geometry tests sweep: one wider than the modal, one that squeezes it.
const SCREEN_WIDTHS: [f32; 2] = [900.0, 620.0];
/// Frames run before measuring, past the window's fade-in.
const FRAMES: usize = 20;
/// Slack for sub-pixel rounding.
const EPS: f32 = 0.5;
/// How far egui's grid stripes reach past the grid on each side.
const STRIPE_REACH: f32 = 2.0;

fn admin() -> User {
    let mut value = serde_json::to_value(User::default()).expect("serialize user");
    value["authorization"] = "Root".into();
    value["email"] = "logan@example.com".into();
    serde_json::from_value(value).expect("deserialize user")
}

fn task(status: Status) -> LiveTaskPayload {
    LiveTaskPayload {
        task_name: "Dana Whitfield - 2000002".to_owned(),
        service_number: Some("2000002".to_owned()),
        status,
        task_description: "Fans ramp and the screen stays black after a restart.".to_owned(),
        ..Default::default()
    }
}

fn ticket() -> TicketData {
    TicketData {
        tech: "checkinshelf".to_owned(),
        terms: "Net 0".to_owned(),
        ticket_total: "149.99".to_owned(),
        checkin_notes: "No POST after a BIOS update.".to_owned(),
        ..Default::default()
    }
}

/// A customer whose name and email are longer than a value cell.
fn customer() -> CustomerData {
    CustomerData {
        cust_code: "88412".to_owned(),
        name: "Dana Whitfield-Montgomery of the Northern Valley".to_owned(),
        phone_number: "801-555-0147".to_owned(),
        email: "dana.whitfield-montgomery.northern.valley@example.com".to_owned(),
        ..Default::default()
    }
}

fn history() -> TaskHistoryLoad {
    let row = |user: &str, diff: serde_json::Value| TaskHistory {
        user: RecordId::new("user", user),
        username: format!("{user}.tech"),
        diff,
        from_writer: true,
        ..Default::default()
    };
    TaskHistoryLoad {
        rows: vec![
            row(
                "joshua",
                serde_json::json!({ "status": { "old": "follow up", "new": "REPO" } }),
            ),
            row(
                "logan",
                serde_json::json!({ "assignee": { "old": "gone", "new": "logan" } }),
            ),
        ],
        usernames: [("gone".to_owned(), "former.tech".to_owned())].into(),
    }
}

/// A task modal on `page` whose async loads never run; needs an entered tokio runtime.
fn modal(status: Status, page: ModalAction) -> ModalType {
    let mut modal = TaskModal::new(ChatView::default(), task(status));
    modal.user = admin();
    modal.current_page_state = page;
    modal.service_ticket = Some(ticket());
    modal.customer = Some(customer());
    modal.task_history = history();
    ModalType::TaskModal(modal)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
}

fn headless(screen_w: f32) -> Headless {
    let h = Headless::new(vec2(screen_w, SCREEN_H), 1.0);
    h.theme(PresetStyles::MtechStudio, Density::Regular);
    h
}

/// One frame of the modal window on the root `Ui`.
fn show(ui: &mut Ui, modal: &mut ModalType) {
    let ctx: Context = ui.ctx().clone();
    let title = modal.to_string();
    modal.ui(&ctx, title, 750.0, 850.0);
}

/// A shape's visible bounds, what it is, and whether it is a grid stripe.
#[derive(Clone, Debug)]
struct Painted {
    rect: Rect,
    what: String,
    stripe: bool,
}

/// The last of [`FRAMES`] frames: card frames, every other shape, and the card inset.
struct Capture {
    cards: Vec<Rect>,
    painted: Vec<Painted>,
    /// Inner margin plus stroke width of a card on each side.
    inset: f32,
}

impl Capture {
    /// Cards strictly inside `within`, top to bottom.
    fn cards_in(&self, within: Rect) -> Vec<Rect> {
        let mut cards: Vec<Rect> = self
            .cards
            .iter()
            .copied()
            .filter(|r| r.left() > within.left() + 1.0 && r.right() < within.right() - 1.0)
            .collect();
        cards.sort_by(|a, b| a.top().total_cmp(&b.top()));
        cards
    }

    /// Shapes whose centre lies in `card`, other than the card frame and what encloses it.
    fn inside(&self, card: Rect) -> impl Iterator<Item = Painted> + '_ {
        self.painted
            .iter()
            .filter(move |p| card.contains(p.rect.center()) && !p.rect.contains_rect(card))
            .cloned()
    }

    /// `card` less its inner margin and stroke on the left and right.
    fn inner(&self, card: Rect) -> Rect {
        card.shrink2(vec2(self.inset, 0.0))
    }

    /// Shapes in `card` lying wholly right of its inner rect: a scroll bar.
    fn bar_in(&self, card: Rect) -> bool {
        let inner = self.inner(card);
        self.inside(card)
            .any(|p| p.rect.left() >= inner.right() - EPS)
    }

    /// The first shape in `card` that leaves its inner rect; stripes may reach [`STRIPE_REACH`] further.
    fn escapee(&self, card: Rect) -> Option<Painted> {
        let inner = self.inner(card);
        self.inside(card).find(|p| {
            let reach = if p.stripe { STRIPE_REACH + EPS } else { EPS };
            p.rect.left() < inner.left() - reach
                || (p.rect.right() > inner.right() + reach && p.rect.left() < inner.right() - EPS)
        })
    }

    /// Right edge of the widest non-stripe shape in `card` short of a scroll bar.
    fn content_right(&self, card: Rect) -> f32 {
        let inner = self.inner(card);
        self.inside(card)
            .filter(|p| !p.stripe && p.rect.left() < inner.right() - EPS)
            .map(|p| p.rect.right())
            .fold(f32::MIN, f32::max)
    }
}

/// Every shape in `shape`, flattened, as `Painted` clipped to `clip`.
fn collect(
    shape: &Shape,
    clip: Rect,
    stripe: Color32,
    card: (Color32, Stroke),
    capture: &mut Capture,
) {
    match shape {
        Shape::Vec(shapes) => shapes
            .iter()
            .for_each(|s| collect(s, clip, stripe, card, capture)),
        Shape::Noop => {}
        // Scroll-area edge fades.
        Shape::Mesh(mesh) if mesh.texture_id == TextureId::default() => {}
        Shape::Rect(r) if r.fill == card.0 && r.stroke == card.1 => capture.cards.push(r.rect),
        other => {
            let rect = other.visual_bounding_rect().intersect(clip);
            if rect.is_positive() {
                let stripe =
                    matches!(other, Shape::Rect(r) if r.fill == stripe && r.stroke.is_empty());
                let what = match other {
                    Shape::Text(text) => format!("text {:?}", text.galley.text()),
                    other => format!("{other:?}").chars().take(60).collect(),
                };
                capture.painted.push(Painted { rect, what, stripe });
            }
        }
    }
}

/// Runs `show` for [`FRAMES`] frames on a `screen_w`-wide screen and captures the last.
fn capture(h: &Headless, screen_w: f32, mut show: impl FnMut(&mut Ui)) -> Capture {
    let mut shapes: Vec<ClippedShape> = Vec::new();
    let mut style = None;
    for _ in 0..FRAMES {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(screen_w, SCREEN_H))),
            ..Default::default()
        };
        let mut full = h.ctx.run_ui(input, |ui| {
            let frame = glass_card::card_frame(ui);
            style = Some((
                ui.visuals().faint_bg_color,
                (glass_card::card_fill(ui), glass_card::card_stroke(ui)),
                f32::from(frame.inner_margin.right) + frame.stroke.width,
            ));
            show(ui);
        });
        // epaint 0.36 panics on a dropped `TexturesDelta` no renderer consumed.
        full.textures_delta.clear();
        shapes = full.shapes;
    }
    let (stripe, card, inset) = style.expect("one frame");
    let mut out = Capture {
        cards: Vec::new(),
        painted: Vec::new(),
        inset,
    };
    for clipped in &shapes {
        collect(&clipped.shape, clipped.clip_rect, stripe, card, &mut out);
    }
    out
}

/// The window rect of the last frame.
fn window(h: &Headless) -> Rect {
    let layer = h.ctx.top_layer_id().expect("window layer");
    h.ctx
        .memory(|m| m.area_rect(layer.id))
        .expect("window rect")
}

/// Asserts `cards` share their left and right edges, sit centred in `window`, and hold their contents.
fn assert_inset_cards(case: &str, capture: &Capture, window: Rect, cards: &[Rect]) {
    let Some(first) = cards.first() else {
        panic!("{case}: no cards");
    };
    for card in cards {
        assert!(
            (card.left() - first.left()).abs() <= 0.01
                && (card.right() - first.right()).abs() <= 0.01,
            "{case}: card spans {:.1}..{:.1}, first card {:.1}..{:.1}",
            card.left(),
            card.right(),
            first.left(),
            first.right()
        );
        if let Some(p) = capture.escapee(*card) {
            panic!(
                "{case}: {} at {:?} leaves the inner rect {:?} of card {card:?}",
                p.what,
                p.rect,
                capture.inner(*card)
            );
        }
    }
    let left = first.left() - window.left();
    let right = window.right() - first.right();
    assert!(
        left > 0.0 && (left - right).abs() <= 1.0,
        "{case}: left gutter {left:.1}, right gutter {right:.1}"
    );
}

/// Asserts the notes card's widest content ends where the info card's inner rect does.
fn assert_notes_match_info(case: &str, capture: &Capture, info: Rect, notes: Rect) {
    let want = capture.inner(info).right();
    let got = capture.content_right(notes);
    assert!(
        (got - want).abs() <= EPS,
        "{case}: notes content ends at {got:.1}, info card content at {want:.1}"
    );
}

#[test]
fn ticket_page_cards_share_edges_sit_centred_and_hold_their_contents() {
    let rt = runtime();
    let _guard = rt.enter();
    for screen_w in SCREEN_WIDTHS {
        for status in [Status::Qc, Status::InRepair] {
            let h = headless(screen_w);
            let mut modal = modal(status.clone(), ModalAction::TicketInfoPage);
            let capture = capture(&h, screen_w, |ui| show(ui, &mut modal));
            let window = window(&h);
            let case = format!("screen {screen_w}, {status:?}, window {window:?}");
            let cards = capture.cards_in(window);
            let &[info, notes] = cards.as_slice() else {
                panic!("{case}: want the info and notes cards, found {cards:?}");
            };
            assert_inset_cards(&case, &capture, window, &cards);
            assert!(
                !capture.bar_in(info),
                "{case}: nothing sits right of the info card's content"
            );
            assert!(
                capture.bar_in(notes),
                "{case}: the notes card should scroll here"
            );
            assert_notes_match_info(&case, &capture, info, notes);
        }
    }
}

#[test]
fn a_notes_card_without_a_scroll_bar_matches_the_info_card() {
    let rt = runtime();
    let _guard = rt.enter();
    for status in [Status::Qc, Status::InRepair] {
        let h = headless(SCREEN_WIDTHS[0]);
        let (mut task, mut ticket, mut customer) = (task(status.clone()), ticket(), customer());
        let (mut seb_checking, mut modal_open, mut history_open, mut shelf) =
            (false, false, false, String::new());
        let page = Rect::from_min_size(pos2(20.0, 20.0), vec2(695.0, SCREEN_H - 40.0));
        let capture = capture(&h, SCREEN_WIDTHS[0], |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(page), |ui| {
                display_ticket_page(
                    ui,
                    &mut task,
                    Some(&mut ticket),
                    Some(&mut customer),
                    None,
                    vec2(700.0, 700.0),
                    &Vec::new(),
                    admin(),
                    None,
                    &mut seb_checking,
                    Some(&mut modal_open),
                    Some(&mut history_open),
                    Some(&mut shelf),
                );
            });
        });
        let case = format!("{status:?}");
        let cards = capture.cards_in(page.expand(1.5));
        let &[info, notes] = cards.as_slice() else {
            panic!("{case}: want the info and notes cards, found {cards:?}");
        };
        assert!(
            !capture.bar_in(notes),
            "{case}: the notes card should not scroll here"
        );
        for card in [info, notes] {
            if let Some(p) = capture.escapee(card) {
                panic!(
                    "{case}: {} at {:?} leaves the inner rect of card {card:?}",
                    p.what, p.rect
                );
            }
        }
        assert_notes_match_info(&case, &capture, info, notes);
    }
}

#[test]
fn computer_and_software_pages_sit_centred_and_hold_their_contents() {
    let rt = runtime();
    let _guard = rt.enter();
    for screen_w in SCREEN_WIDTHS {
        for page in [ModalAction::ComputerInfoPage, ModalAction::SoftwareInfoPage] {
            let h = headless(screen_w);
            let mut modal = modal(Status::InRepair, page.clone());
            let capture = capture(&h, screen_w, |ui| show(ui, &mut modal));
            let window = window(&h);
            let case = format!("screen {screen_w}, {page:?}, window {window:?}");
            let cards = capture.cards_in(window);
            assert_inset_cards(&case, &capture, window, &cards);
            for card in cards {
                assert!(
                    !capture.bar_in(card),
                    "{case}: nothing sits right of {card:?}'s content"
                );
            }
        }
    }
}

/// Writes every page's render; run with `-- --ignored render_task_modal_pages`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_task_modal_pages() {
    let dir = std::env::var_os("MTECH_RENDER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mtech-renders"));
    std::fs::create_dir_all(&dir).expect("render directory");
    let rt = runtime();
    let _guard = rt.enter();
    let pages = [
        (Status::Qc, ModalAction::TicketInfoPage, "ticket_qc"),
        (
            Status::InRepair,
            ModalAction::TicketInfoPage,
            "ticket_repair",
        ),
        (Status::InRepair, ModalAction::ComputerInfoPage, "computer"),
        (Status::InRepair, ModalAction::SoftwareInfoPage, "software"),
        (Status::InRepair, ModalAction::TaskHistoryPage, "history"),
        (
            Status::InRepair,
            ModalAction::DiagnosticsPage,
            "diagnostics",
        ),
        (Status::InRepair, ModalAction::StressTestsPage, "stress"),
        (Status::InRepair, ModalAction::TaskNotePage, "notes"),
    ];
    let save = |name: &str, h: &Headless, prims: &[ClippedPrimitive]| {
        h.image(prims)
            .save(dir.join(format!("task_modal_{name}.png")))
            .expect("write png");
    };
    for (status, page, name) in pages {
        let mut h = headless(SCREEN_WIDTHS[0]);
        let mut modal = modal(status, page);
        let mut prims = Vec::new();
        for _ in 0..FRAMES {
            prims = h.frame_root(Vec::new(), |ui| show(ui, &mut modal)).1;
        }
        save(name, &h, &prims);
    }
    let narrow = [
        (Status::Qc, ModalAction::TicketInfoPage, "ticket_qc_narrow"),
        (
            Status::InRepair,
            ModalAction::TicketInfoPage,
            "ticket_repair_narrow",
        ),
        (
            Status::InRepair,
            ModalAction::ComputerInfoPage,
            "computer_narrow",
        ),
        (
            Status::InRepair,
            ModalAction::SoftwareInfoPage,
            "software_narrow",
        ),
    ];
    for (status, page, name) in narrow {
        let mut h = headless(SCREEN_WIDTHS[1]);
        let mut modal = modal(status, page);
        let mut prims = Vec::new();
        for _ in 0..FRAMES {
            prims = h.frame_root(Vec::new(), |ui| show(ui, &mut modal)).1;
        }
        save(name, &h, &prims);
    }
}
