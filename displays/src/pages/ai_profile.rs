//! Account-settings section for the signed-in user's AI assistant persona and their task schedules.

use std::sync::Mutex;

use database::schema::task_schedule::{TaskSchedule, local_label};
use database::schema::{AiProfile, RecordId, RecordIdExt, User};
use eframe::egui::{RichText, ScrollArea, Ui};
use once_cell::sync::Lazy;

use crate::ui_tools::controls::{
    self, ButtonKind, IconLabel, Metrics, SpinnerSize, TextArea, TextButton, TextField, Tokens,
};
use crate::ui_tools::glass_card;
use crate::ui_tools::icons::svg;
use crate::{PlatformSpawner, Spawner};

/// Width of the editor's fields.
const FIELD_W: f32 = 330.0;
/// Widest the editor card grows.
const CARD_W: f32 = 360.0;
/// Width of the Answers dropdown.
const DETAIL_W: f32 = 120.0;
/// Longest assistant name.
const NAME_CHARS: usize = 40;

/// Editor state shared with the async loads and saves.
#[derive(Default)]
struct Editor {
    loaded_for: Option<RecordId>,
    name: String,
    personality: String,
    detail: String,
    about: String,
    morning_brief: bool,
    status: Option<(bool, String)>,
    schedules: Vec<TaskSchedule>,
    busy: bool,
}

static EDITOR: Lazy<Mutex<Editor>> = Lazy::new(|| Mutex::new(Editor::default()));

fn with_editor<R>(f: impl FnOnce(&mut Editor) -> R) -> Option<R> {
    EDITOR.lock().ok().map(|mut e| f(&mut e))
}

fn load(user: RecordId) {
    PlatformSpawner::spawn(async move {
        let profile = AiProfile::for_user(&user).await.unwrap_or_else(|e| {
            log::warn!("ai profile: load failed: {e}");
            None
        });
        let schedules = TaskSchedule::for_user(&user).await.unwrap_or_else(|e| {
            log::warn!("ai profile: schedules load failed: {e}");
            Vec::new()
        });
        with_editor(|ed| {
            let p = profile.unwrap_or_default();
            ed.name = p.assistant_name.clone().unwrap_or_default();
            ed.personality = p.personality.clone().unwrap_or_default();
            ed.detail = p.detail.clone().unwrap_or_default();
            ed.about = p.about_me.clone().unwrap_or_default();
            ed.morning_brief = p.wants_morning_brief();
            ed.schedules = schedules;
            ed.busy = false;
        });
    });
}

fn save(profile: AiProfile) {
    PlatformSpawner::spawn(async move {
        let result = User::save_ai_profile(profile).await;
        with_editor(|ed| {
            ed.busy = false;
            ed.status = Some(match result {
                Ok(_) => (
                    true,
                    "Saved. New AI sessions use it right away.".to_string(),
                ),
                Err(e) if e.to_string().contains("no such field exists") => (
                    false,
                    "Not saved: AI profiles are not enabled on the server yet.".to_string(),
                ),
                Err(e) => (false, format!("Not saved: {e}")),
            });
        });
    });
}

fn cancel(user: RecordId, schedule: RecordId) {
    PlatformSpawner::spawn(async move {
        if let Err(e) = TaskSchedule::cancel(&schedule).await {
            with_editor(|ed| ed.status = Some((false, format!("Could not cancel: {e}"))));
        }
        load(user);
    });
}

fn opt(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// The AI assistant persona editor and the user's schedules.
pub fn ai_profile_section(ui: &mut Ui, user: &User) {
    let user_id = user.get_id();
    let Ok(mut ed) = EDITOR.lock() else { return };
    if ed.loaded_for.as_ref() != Some(&user_id) {
        *ed = Editor {
            loaded_for: Some(user_id.clone()),
            busy: true,
            morning_brief: true,
            ..Default::default()
        };
        load(user_id.clone());
    }
    ScrollArea::vertical()
        .id_salt("ai_profile_section")
        .max_height(460.0)
        .auto_shrink([false, true])
        .show(ui, |ui| editor_ui(ui, &mut ed, &user_id));
}

fn editor_ui(ui: &mut Ui, ed: &mut Editor, user_id: &RecordId) {
    let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
    glass_card::group(ui, |ui| {
        ui.set_max_width(CARD_W);
        glass_card::card_header(ui, svg::ROBOT, "AI assistant", None);
        controls::section_label(ui, "Name");
        TextField::new(&mut ed.name)
            .hint("e.g. Jarvis")
            .icon(svg::IDENTIFICATION_BADGE)
            .width(FIELD_W)
            .show(ui);
        if ed.name.chars().count() > NAME_CHARS {
            ed.name = ed.name.chars().take(NAME_CHARS).collect();
        }
        ui.add_space(m.gap.y);
        controls::section_label(ui, "Personality");
        TextArea::new(&mut ed.personality)
            .hint("e.g. dry humor, straight to the point")
            .rows(2)
            .width(FIELD_W)
            .char_limit(300)
            .show(ui);
        ui.add_space(m.gap.y);
        ui.horizontal(|ui| {
            controls::icon_label(ui, svg::TEXT_AA, "Answers");
            let options = [
                (String::new(), "Normal"),
                ("brief".to_string(), "Brief"),
                ("detailed".to_string(), "Detailed"),
            ];
            controls::dropdown(ui, "ai_profile_detail", &mut ed.detail, &options, DETAIL_W);
        });
        ui.add_space(m.gap.y);
        controls::section_label(ui, "About me");
        TextArea::new(&mut ed.about)
            .hint("role, experience, what it should know about you")
            .rows(3)
            .width(FIELD_W)
            .char_limit(500)
            .show(ui);
        ui.add_space(m.gap.y);
        controls::toggle(ui, &mut ed.morning_brief, "Morning brief at store open");
        ui.add_space(m.gap.y);
        ui.horizontal(|ui| {
            let save_button = TextButton::new("Save assistant")
                .icon(svg::FLOPPY_DISK)
                .kind(ButtonKind::Primary)
                .prominent(true)
                .enabled(!ed.busy)
                .show(ui);
            if save_button.clicked() {
                let profile = AiProfile {
                    assistant_name: opt(&ed.name),
                    personality: opt(&ed.personality),
                    detail: opt(&ed.detail),
                    about_me: opt(&ed.about),
                    morning_brief: Some(ed.morning_brief),
                };
                ed.busy = true;
                ed.status = None;
                save(profile);
            }
            if ed.busy {
                controls::spinner(ui, SpinnerSize::Sm);
            }
        });
        if let Some((ok, msg)) = &ed.status {
            let (icon, color) = if *ok {
                (svg::CHECK_CIRCLE, t.success)
            } else {
                (svg::WARNING_CIRCLE, t.error)
            };
            IconLabel::new(icon, msg)
                .tone(color)
                .small(true)
                .wrap(true)
                .show(ui);
        }
    });

    ui.add_space(m.gap.y);
    controls::subheading(ui, svg::CALENDAR_CHECK, "My schedules");
    if ed.schedules.is_empty() {
        controls::empty_state(
            ui,
            svg::CALENDAR_BLANK,
            "No schedules",
            "Ask the AI, e.g. \"remind me every Monday at 10 to count thermal paste\".",
            &[],
        );
        return;
    }
    let mut cancel_id = None;
    for s in &ed.schedules {
        ui.horizontal_wrapped(|ui| {
            let next = s
                .next_run
                .map(|t| local_label(t.into_inner()))
                .unwrap_or_default();
            controls::icon_label(ui, svg::CLOCK_CLOCKWISE, &s.title);
            ui.label(
                RichText::new(format!("{} · next {next}", s.describe()))
                    .small()
                    .weak(),
            );
            let cancel = TextButton::new("Cancel")
                .icon(svg::TRASH)
                .kind(ButtonKind::Ghost)
                .show(ui)
                .on_hover_text(s.id.key_string());
            if cancel.clicked() {
                cancel_id = Some(s.id.clone());
            }
        });
    }
    if let Some(id) = cancel_id {
        ed.busy = true;
        cancel(user_id.clone(), id);
    }
}
