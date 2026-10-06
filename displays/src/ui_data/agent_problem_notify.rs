//! Toasts an active Root user about each new AI problem outside their own requests.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crossbeam::channel::{Receiver, Sender, unbounded};
use database::schema::{AgentProblem, ApprovalViewer, RecordId, User};
use eframe::egui::{Align, Context, Frame, Layout, Margin, Response, RichText, Sense, Ui, Vec2};
use web_time::Instant;

use crate::ui_tools::toasts::{Toast, ToastKind, ToastOptions, Toasts};
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};

/// `ToastKind::Custom` discriminant for AI-problem toasts.
pub const PROBLEM_TOAST_KIND: u32 = 0x7A5C_0004;

const POLL_EVERY: Duration = Duration::from_secs(20);
/// Window each poll reads; a problem already older than this when first read never toasts.
const POLL_WINDOW: &str = "15m";
/// New problems in one read past this count share one toast.
const MAX_SINGLE_TOASTS: usize = 3;
/// How recently the Problems list must have been drawn to count as on screen.
const IN_VIEW_FOR: Duration = Duration::from_millis(1500);
const TOAST_WIDTH: f32 = 340.0;

/// What one toast shows.
#[derive(Debug, Clone)]
enum Notice {
    One(Box<AgentProblem>),
    Many(usize),
}

/// What a toast's button asked the Ai tab to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemsRequest {
    /// The Problems list.
    List,
    /// One problem without a session, by key.
    Problem(String),
}

/// State the Problems list and the toast renderer share with the notifier.
#[derive(Default)]
struct Board {
    notices: HashMap<String, Notice>,
    list_drawn: Option<Instant>,
    request: Option<ProblemsRequest>,
}

static BOARD: LazyLock<Mutex<Board>> = LazyLock::new(Mutex::default);

fn with_board<R>(f: impl FnOnce(&mut Board) -> R) -> Option<R> {
    BOARD.lock().ok().map(|mut board| f(&mut board))
}

/// Records that the Problems list was drawn this frame.
pub fn mark_list_in_view() {
    with_board(|b| b.list_drawn = Some(Instant::now()));
}

/// What a toast's button asked the Ai tab to show, handed out once.
pub fn take_problems_request() -> Option<ProblemsRequest> {
    with_board(|b| b.request.take()).flatten()
}

/// True while any viewport of the app has focus and the Problems list is on screen.
fn list_in_view(ctx: &Context) -> bool {
    let focused =
        ctx.input(|i| i.raw.focused || i.raw.viewports.values().any(|v| v.focused == Some(true)));
    focused
        && with_board(|b| b.list_drawn.is_some_and(|t| t.elapsed() < IN_VIEW_FOR)).unwrap_or(false)
}

/// The requester's email name and store, for a toast's subject line.
fn who(problem: &AgentProblem) -> String {
    let name = problem
        .requested_by
        .as_deref()
        .map(|email| email.split('@').next().unwrap_or(email).to_string())
        .unwrap_or_else(|| "Unattributed".to_string());
    match problem.store.as_deref() {
        Some(store) => format!("{name} ({store})"),
        None => name,
    }
}

type Read = (u64, Result<Vec<AgentProblem>, String>);

/// Reads recent AI problems for an active Root user and toasts the new ones.
pub struct AgentProblemNotifier {
    /// The active Root user signed in, with their email.
    viewer: Option<(RecordId, String)>,
    /// Keys of the problems in the last read.
    seen: HashSet<String>,
    /// False until the first read, whose problems count as already known.
    primed: bool,
    /// Bumped on every sign-in change; reads for an earlier viewer are dropped.
    generation: u64,
    loading: bool,
    last_poll: Option<Instant>,
    tx: Sender<Read>,
    rx: Receiver<Read>,
}

impl Default for AgentProblemNotifier {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            viewer: None,
            seen: HashSet::new(),
            primed: false,
            generation: 0,
            loading: false,
            last_poll: None,
            tx,
            rx,
        }
    }
}

impl AgentProblemNotifier {
    /// Applies finished reads, toasts new problems and starts the next read; idle unless an active Root is signed in.
    pub fn tick(&mut self, ctx: &Context, user: Option<&User>, toasts: &mut Toasts) {
        let viewer = user.and_then(|u| {
            ApprovalViewer::of(u)
                .filter(|v| v.root)
                .map(|v| (v.id, u.get_email().trim().to_string()))
        });
        if viewer.as_ref().map(|v| &v.0) != self.viewer.as_ref().map(|v| &v.0) {
            self.reset(viewer);
        }
        let Some((_, email)) = self.viewer.clone() else {
            return;
        };

        while let Ok((generation, read)) = self.rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            self.loading = false;
            match read {
                Ok(problems) => {
                    let fresh = self.take_new(problems, &email);
                    if !fresh.is_empty() && !list_in_view(ctx) {
                        show(fresh, toasts);
                    }
                }
                Err(e) => log::debug!("agent problem read failed: {e}"),
            }
        }

        if !self.loading && self.last_poll.is_none_or(|t| t.elapsed() >= POLL_EVERY) {
            self.poll(ctx);
        }
        ctx.request_repaint_after(POLL_EVERY);
    }

    fn reset(&mut self, viewer: Option<(RecordId, String)>) {
        self.viewer = viewer;
        self.seen.clear();
        self.primed = false;
        self.generation += 1;
        self.loading = false;
        self.last_poll = None;
        with_board(|b| {
            b.notices.clear();
            b.request = None;
        });
    }

    fn poll(&mut self, ctx: &Context) {
        self.loading = true;
        self.last_poll = Some(Instant::now());
        let tx = self.tx.clone();
        let generation = self.generation;
        let ctx = ctx.clone();
        PlatformSpawner::spawn(async move {
            let read = AgentProblem::list(POLL_WINDOW)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send((generation, read));
            ctx.request_repaint();
        });
    }

    /// Keeps the keys of `problems`; returns the ones missing from the last read, other than `email`'s, once primed.
    fn take_new(&mut self, problems: Vec<AgentProblem>, email: &str) -> Vec<AgentProblem> {
        let primed = std::mem::replace(&mut self.primed, true);
        let keys: HashSet<String> = problems.iter().map(|p| p.key.clone()).collect();
        let fresh = problems
            .into_iter()
            .filter(|p| primed && !self.seen.contains(&p.key) && !p.requested_by_user(email))
            .collect();
        self.seen = keys;
        fresh
    }
}

/// One toast per problem, or one for all of them past [`MAX_SINGLE_TOASTS`].
fn show(fresh: Vec<AgentProblem>, toasts: &mut Toasts) {
    if fresh.len() > MAX_SINGLE_TOASTS {
        let key = format!("many:{}", fresh[0].key);
        add_toast(toasts, key, Notice::Many(fresh.len()));
        return;
    }
    for problem in fresh {
        add_toast(toasts, problem.key.clone(), Notice::One(Box::new(problem)));
    }
}

fn add_toast(toasts: &mut Toasts, key: String, notice: Notice) {
    with_board(|b| b.notices.insert(key.clone(), notice));
    toasts.add(Toast {
        kind: ToastKind::Custom(PROBLEM_TOAST_KIND),
        text: format!("ai problem {key}").into(),
        options: ToastOptions::default()
            .show_icon(false)
            .show_progress(false),
        payload: Some(key),
        ..Default::default()
    });
}

/// Draws an AI-problem toast; registered on the shared [`Toasts`] under [`PROBLEM_TOAST_KIND`].
pub fn toast_contents(ui: &mut Ui, toast: &mut Toast) -> Response {
    let key = toast.payload.clone().unwrap_or_default();
    let Some(notice) = with_board(|b| b.notices.get(&key).cloned()).flatten() else {
        toast.close();
        return ui.allocate_response(Vec2::ZERO, Sense::hover());
    };

    let (title, subject, detail, button) = match &notice {
        Notice::One(p) => (
            format!(
                "{} AI problem \u{00b7} {}",
                icons::STATUS_ERR,
                p.kind.label()
            ),
            Some(format!("{} \u{00b7} {}", who(p), p.subject())),
            p.headline(),
            if p.thread.is_some() {
                "Open session"
            } else {
                "View problem"
            },
        ),
        Notice::Many(n) => (
            format!("{} {n} new AI problems", icons::STATUS_ERR),
            None,
            "Several requests or sessions failed at once.".to_string(),
            "View problems",
        ),
    };
    let mut open = false;
    let mut dismiss = false;
    let response = Frame::window(ui.style())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(TOAST_WIDTH);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&title).strong().color(theme::error(ui)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    dismiss = ui
                        .small_button(icons::CLOSE)
                        .on_hover_text("Dismiss")
                        .clicked();
                });
            });
            if let Some(subject) = &subject {
                ui.label(RichText::new(subject).small().color(theme::weak_text(ui)));
            }
            ui.add_space(2.0);
            ui.label(&detail);
            ui.add_space(4.0);
            open = ui
                .button(format!("{} {button}", icons::ARROW_RIGHT))
                .clicked();
        })
        .response;

    if open {
        match &notice {
            Notice::One(p) => match &p.thread {
                Some(thread) => {
                    super::agent_session_notify::request_open(thread.clone(), p.thread_is_open())
                }
                None => {
                    with_board(|b| b.request = Some(ProblemsRequest::Problem(p.key.clone())));
                }
            },
            Notice::Many(_) => {
                with_board(|b| b.request = Some(ProblemsRequest::List));
            }
        }
    }
    if open || dismiss {
        with_board(|b| b.notices.remove(&key));
        toast.close();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::ProblemKind;

    fn problem(key: &str, by: &str) -> AgentProblem {
        AgentProblem {
            key: key.to_string(),
            kind: ProblemKind::AgentError,
            at: None,
            requested_by: Some(by.to_string()),
            store: Some("LTN".to_string()),
            hostname: Some("PC-1".to_string()),
            connection_string: "PC-1:abc".to_string(),
            service_number: None,
            title: None,
            thread: None,
            thread_status: None,
            request: None,
            message: "Agent error: exceeded retry limit, last status: 429 Too Many Requests"
                .to_string(),
        }
    }

    fn keys(problems: &[AgentProblem]) -> Vec<&str> {
        problems.iter().map(|p| p.key.as_str()).collect()
    }

    #[test]
    fn the_first_read_is_quiet_and_later_reads_report_only_new_keys() {
        let mut n = AgentProblemNotifier::default();
        let first = n.take_new(vec![problem("a", "tech@x.com")], "root@x.com");
        assert!(first.is_empty(), "problems known at sign-in do not toast");
        let second = n.take_new(
            vec![problem("a", "tech@x.com"), problem("b", "tech@x.com")],
            "root@x.com",
        );
        assert_eq!(keys(&second), ["b"]);
        let third = n.take_new(vec![problem("b", "tech@x.com")], "root@x.com");
        assert!(third.is_empty());
    }

    #[test]
    fn the_viewers_own_problems_do_not_toast() {
        let mut n = AgentProblemNotifier::default();
        n.take_new(Vec::new(), "root@x.com");
        let fresh = n.take_new(
            vec![problem("a", "Root@X.com"), problem("b", "tech@x.com")],
            "root@x.com",
        );
        assert_eq!(keys(&fresh), ["b"]);
    }

    #[test]
    fn a_problem_that_left_the_window_and_returns_toasts_again() {
        let mut n = AgentProblemNotifier::default();
        n.take_new(vec![problem("a", "tech@x.com")], "root@x.com");
        n.take_new(Vec::new(), "root@x.com");
        let back = n.take_new(vec![problem("a", "tech@x.com")], "root@x.com");
        assert_eq!(keys(&back), ["a"]);
    }

    #[test]
    fn the_subject_names_the_requester_and_store() {
        assert_eq!(
            who(&problem("a", "ethan.thomas@pclaptops.com")),
            "ethan.thomas (LTN)"
        );
        let mut p = problem("a", "x");
        p.requested_by = None;
        p.store = None;
        assert_eq!(who(&p), "Unattributed");
    }
}
