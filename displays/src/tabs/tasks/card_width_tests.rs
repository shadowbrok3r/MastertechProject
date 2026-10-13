//! Board card lists: every card in a column shares the column's left and right edges.

use crossbeam::channel::{Sender, unbounded};
use database::schema::{
    AiTask, AiTaskStatus, ConnectedClient, LiveTaskPayload, Priority, Status, SystemInformation,
    User,
};
use eframe::egui::{
    Event, Modifiers, PointerButton, Pos2, Rect, ScrollArea, Ui, epaint::ClippedPrimitive, pos2,
    vec2,
};

use crate::tabs::tasks::ai_task_cards::{AiCardRole, AiTaskCardView};
use crate::tabs::tasks::client_cards::ClientCardData;
use crate::tabs::tasks::task_cards::RecommendationSummary;
use crate::ui_tools::{
    headless_render::Headless, metrics::Density, metrics::Metrics, theme_config::PresetStyles,
};
use crate::{Displayable, TaskUiActions};

/// Content width of a real board column's card scroll area.
const BOARD_COL_W: f32 = 430.0;
/// Column widths the geometry test sweeps.
const WIDTHS: [f32; 4] = [BOARD_COL_W, 300.0, 360.0, 640.0];
/// Screen height for the column renders.
const SCREEN_H: f32 = 1600.0;
/// Margin around the column on the headless screen.
const PAD: f32 = 20.0;

fn user(email: &str) -> User {
    let mut u = User::default();
    u.set_email(email);
    u
}

struct Board {
    me: User,
    users: Vec<User>,
    tasks: Vec<LiveTaskPayload>,
    ai_cards: Vec<AiTaskCardView>,
    clients: Vec<ClientCardData>,
    tx: Sender<TaskUiActions>,
}

impl Board {
    fn new() -> Self {
        let me = user("al@example.com");
        let long = user("christopher.montgomery-smith@example.com");
        let users = vec![me.clone(), long.clone()];

        let task =
            |name: &str, status: Status, priority: Priority, assignee: &User| LiveTaskPayload {
                task_name: name.to_owned(),
                status,
                priority,
                assignee: assignee.get_id(),
                ..Default::default()
            };
        let mut tasks = vec![
            task("Ann Lee - 1001", Status::Todo, Priority::Normal, &me),
            task(
                "Christopher Montgomery-Smith - 2154905 - no POST after a BIOS update",
                Status::InRepair,
                Priority::Express,
                &long,
            ),
            task(
                "Bo - 7",
                Status::CustomStatus("Waiting on customer approval".to_owned()),
                Priority::Rfs,
                &User::default(),
            ),
            task("Dana Whitfield - 2155144", Status::Qc, Priority::Fire, &me),
            task("", Status::Complete, Priority::Qc, &long),
        ];
        tasks[3].task_description = "Customer reports the fans ramp and the screen stays black \
            after a restart. Swap-test the PSU, reseat the RAM and update the BIOS from 1.A5."
            .to_owned();
        tasks[4].completed = true;

        let ai = |title: &str, assignee: &User, status: AiTaskStatus, role: AiCardRole| {
            let ai_task = AiTask {
                title: title.to_owned(),
                customer_name: "Dana Whitfield".to_owned(),
                service_number: "2155144".to_owned(),
                assignee: assignee.get_id(),
                requested_by: me.get_id(),
                status,
                ..Default::default()
            };
            AiTaskCardView {
                ai_task,
                items: Vec::new(),
                linked_task: None,
                role,
                in_grace: false,
            }
        };
        let mut ai_cards = vec![
            ai(
                "Reseat RAM",
                &me,
                AiTaskStatus::Open,
                AiCardRole::AssignedTech,
            ),
            ai(
                "Check the PSU rails under load, then rerun the full stress suite and report temps",
                &long,
                AiTaskStatus::AwaitingFollowup,
                AiCardRole::Operator,
            ),
            ai(
                "Flash BIOS",
                &long,
                AiTaskStatus::Open,
                AiCardRole::AssignedTech,
            ),
        ];
        ai_cards[1].ai_task.current_theory = Some(
            "The 12 V rail sags below spec when the GPU ramps, which matches the restart pattern"
                .to_owned(),
        );

        let client = |name: Option<&str>, conn: &str| {
            ClientCardData::new(ConnectedClient {
                friendly_name: name.map(str::to_owned),
                connection_string: conn.to_owned(),
                connected: true,
                ..Default::default()
            })
        };
        let mut clients = vec![
            client(Some("Bench 3"), "DESKTOP-1:abc"),
            client(None, "DESKTOP-EI5PV29-LONG-HOSTNAME-WORKSTATION:69cd8115b"),
            client(Some("Reception laptop"), "LAPTOP-9:fff"),
        ];
        clients[1].is_ws_connected = true;
        clients[1].ai_active = true;
        clients[2].system_info = Some(SystemInformation {
            hostname: "LAPTOP-9".to_owned(),
            os_version: "Windows 11 Pro 24H2 (26100.4061)".to_owned(),
            cpu_percentage: 37.0,
            used_memory: 6144.0,
            total_memory: 16384.0,
            ..Default::default()
        });

        let (tx, _rx) = unbounded();
        Self {
            me,
            users,
            tasks,
            ai_cards,
            clients,
            tx,
        }
    }

    fn task_cards(&mut self, ui: &mut Ui) -> Vec<Rect> {
        let Self {
            me,
            users,
            tasks,
            tx,
            ..
        } = self;
        tasks
            .iter_mut()
            .map(|task| {
                ui.scope(|ui| {
                    task.display_cards(
                        ui,
                        me,
                        users,
                        Vec::new(),
                        tx.clone(),
                        None,
                        RecommendationSummary::default(),
                        None,
                    )
                })
                .response
                .rect
            })
            .collect()
    }

    fn ai_cards(&self, ui: &mut Ui) -> Vec<Rect> {
        self.ai_cards
            .iter()
            .map(|card| {
                ui.scope(|ui| card.display(ui, &self.me, &self.users, Vec::new(), None, &self.tx))
                    .response
                    .rect
            })
            .collect()
    }

    fn client_cards(&self, ui: &mut Ui) -> Vec<Rect> {
        let gap = Metrics::get(ui.ctx()).gap.y;
        self.clients
            .iter()
            .map(|card| {
                let rect = ui
                    .scope(|ui| card.display_client_card(ui, &self.tx))
                    .response
                    .rect;
                ui.add_space(gap);
                rect
            })
            .collect()
    }
}

/// Lays `cards` out in a `width`-wide scrolling column; returns its content rect and card rects.
fn column(ui: &mut Ui, width: f32, cards: impl FnOnce(&mut Ui) -> Vec<Rect>) -> (Rect, Vec<Rect>) {
    let rect = Rect::from_min_size(pos2(PAD, PAD), vec2(width, SCREEN_H - 2.0 * PAD));
    let mut column = ui.new_child(eframe::egui::UiBuilder::new().max_rect(rect));
    ScrollArea::vertical()
        .id_salt(("card_width_column", width as u32))
        .max_width(width)
        .auto_shrink(false)
        .show(&mut column, |ui| {
            let content = ui.max_rect();
            (content, cards(ui))
        })
        .inner
}

/// Every card's left and right edges against the column's content rect.
fn edge_mismatch(content: Rect, cards: &[Rect]) -> Option<String> {
    let first = cards.first()?;
    let off = |a: f32, b: f32| (a - b).abs() > 0.01;
    for (i, card) in cards.iter().enumerate() {
        if off(card.left(), first.left()) || off(card.right(), first.right()) {
            return Some(format!(
                "card {i} spans {:.1}..{:.1}, card 0 spans {:.1}..{:.1}",
                card.left(),
                card.right(),
                first.left(),
                first.right()
            ));
        }
        if off(card.left(), content.left()) || off(card.right(), content.right()) {
            return Some(format!(
                "card {i} spans {:.1}..{:.1}, column content spans {:.1}..{:.1}",
                card.left(),
                card.right(),
                content.left(),
                content.right()
            ));
        }
    }
    None
}

fn headless(width: f32) -> Headless {
    let h = Headless::new(vec2(width + 2.0 * PAD, SCREEN_H), 1.0);
    h.theme(PresetStyles::MtechStudio, Density::Regular);
    h
}

fn pointer_click(h: &mut Headless, at: Pos2, mut f: impl FnMut(&mut Ui)) {
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.frame_root(vec![Event::PointerMoved(at), press(true)], &mut f);
    h.frame_root(vec![press(false)], &mut f);
}

/// Centre of the closed "Task Description" header, the last row of a task card.
fn description_header(card: Rect, ctx: &eframe::egui::Context) -> Pos2 {
    let header_h = Metrics::get(ctx).header_h;
    pos2(card.left() + 60.0, card.bottom() - 12.0 - header_h / 2.0)
}

/// A column's last settled frame.
struct Settled {
    content: Rect,
    cards: Vec<Rect>,
    prims: Vec<ClippedPrimitive>,
}

/// Runs a few frames of `cards` in a `width`-wide column.
fn settle(h: &mut Headless, width: f32, mut cards: impl FnMut(&mut Ui) -> Vec<Rect>) -> Settled {
    let mut last = None;
    for _ in 0..4 {
        last = Some(h.frame_root(Vec::new(), |ui| column(ui, width, &mut cards)));
    }
    let ((content, cards), prims) = last.expect("at least one frame");
    Settled {
        content,
        cards,
        prims,
    }
}

/// The task column at `width`: card rects with every description closed, then card 3 opened.
fn task_column(width: f32) -> (Headless, Vec<Rect>, Settled) {
    let mut h = headless(width);
    let mut board = Board::new();
    let closed = settle(&mut h, width, |ui| board.task_cards(ui)).cards;
    let click = description_header(closed[3], &h.ctx);
    pointer_click(&mut h, click, |ui| {
        column(ui, width, |ui| board.task_cards(ui));
    });
    let open = settle(&mut h, width, |ui| board.task_cards(ui));
    (h, closed, open)
}

#[test]
fn task_cards_share_the_column_edges_whatever_they_hold() {
    for width in WIDTHS {
        let (_, closed, open) = task_column(width);
        assert!(
            open.cards[3].height() > closed[3].height() + 40.0,
            "width {width}: the description click did not open card 3 ({:.1} -> {:.1})",
            closed[3].height(),
            open.cards[3].height()
        );
        for (label, cards) in [("closed", &closed), ("open", &open.cards)] {
            if let Some(err) = edge_mismatch(open.content, cards) {
                panic!("width {width}, {label} descriptions: {err}");
            }
        }
    }
}

#[test]
fn ai_task_cards_share_the_column_edges_whatever_they_hold() {
    let board = Board::new();
    for width in WIDTHS {
        let column = settle(&mut headless(width), width, |ui| board.ai_cards(ui));
        if let Some(err) = edge_mismatch(column.content, &column.cards) {
            panic!("width {width}: {err}");
        }
    }
}

#[test]
fn client_cards_share_the_column_edges_whatever_they_hold() {
    let board = Board::new();
    for width in WIDTHS {
        let column = settle(&mut headless(width), width, |ui| board.client_cards(ui));
        if let Some(err) = edge_mismatch(column.content, &column.cards) {
            panic!("width {width}: {err}");
        }
    }
}

/// Writes the board-column renders; run with `-- --ignored render_board_columns`.
#[test]
#[ignore = "writes PNG previews; set MTECH_RENDER_DIR"]
fn render_board_columns() {
    let dir = std::env::var_os("MTECH_RENDER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mtech-renders"));
    std::fs::create_dir_all(&dir).expect("render directory");
    let save = |h: &Headless, column: &Settled, name: &str| {
        let image = h.image(&column.prims);
        let bottom = column.cards.last().map_or(SCREEN_H, |r| r.bottom()) + PAD;
        let bottom = bottom.ceil().min(image.height() as f32) as u32;
        image::imageops::crop_imm(&image, 0, 0, image.width(), bottom)
            .to_image()
            .save(dir.join(format!("{name}.png")))
            .expect("write column png");
    };

    for width in [BOARD_COL_W, 300.0] {
        let (h, _, open) = task_column(width);
        save(&h, &open, &format!("board_tasks_{}", width as u32));
    }

    let board = Board::new();
    let width = BOARD_COL_W + 16.0;
    let mut h = headless(width);
    let ai = settle(&mut h, width, |ui| board.ai_cards(ui));
    save(&h, &ai, "board_ai");
    let mut h = headless(width);
    let clients = settle(&mut h, width, |ui| board.client_cards(ui));
    save(&h, &clients, "board_clients");
}
