//! Account-settings section for the signed-in user's AI assistant persona and their task schedules.

use std::sync::Mutex;

use database::schema::task_schedule::{TaskSchedule, local_label};
use database::schema::{AiProfile, RecordId, RecordIdExt, User};
use eframe::egui::{Button, ComboBox, RichText, ScrollArea, TextEdit, Ui};
use once_cell::sync::Lazy;

use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};

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
                Ok(_) => (true, "Saved. New AI sessions use it right away.".to_string()),
                Err(e) if e.to_string().contains("no such field exists") => {
                    (false, "Not saved: AI profiles are not enabled on the server yet.".to_string())
                }
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
        *ed = Editor { loaded_for: Some(user_id.clone()), busy: true, morning_brief: true, ..Default::default() };
        load(user_id.clone());
    }
    ScrollArea::vertical()
        .id_salt("ai_profile_section")
        .max_height(460.0)
        .auto_shrink([false, true])
        .show(ui, |ui| editor_ui(ui, &mut ed, &user_id));
}

fn editor_ui(ui: &mut Ui, ed: &mut Editor, user_id: &RecordId) {
    ui.heading(RichText::new(format!("{} AI Assistant", icons::ROBOT)).strong());
    ui.add_space(6.0);
    ui.group(|ui| {
        ui.set_max_width(360.0);
        ui.label(RichText::new("Name").small().weak());
        TextEdit::singleline(&mut ed.name).hint_text(" e.g. Jarvis").desired_width(330.0).char_limit(40).show(ui);
        ui.add_space(4.0);
        ui.label(RichText::new("Personality").small().weak());
        TextEdit::multiline(&mut ed.personality)
            .hint_text(" e.g. dry humor, straight to the point")
            .desired_rows(2)
            .desired_width(330.0)
            .char_limit(300)
            .show(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Answers").small().weak());
            let shown = match ed.detail.as_str() {
                "brief" => "Brief",
                "detailed" => "Detailed",
                _ => "Normal",
            };
            ComboBox::from_id_salt("ai_profile_detail").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut ed.detail, String::new(), "Normal");
                ui.selectable_value(&mut ed.detail, "brief".to_string(), "Brief");
                ui.selectable_value(&mut ed.detail, "detailed".to_string(), "Detailed");
            });
        });
        ui.add_space(4.0);
        ui.label(RichText::new("About me").small().weak());
        TextEdit::multiline(&mut ed.about)
            .hint_text(" role, experience, what it should know about you")
            .desired_rows(3)
            .desired_width(330.0)
            .char_limit(500)
            .show(ui);
        ui.add_space(4.0);
        ui.checkbox(&mut ed.morning_brief, "Morning brief at store open");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let save_btn =
                Button::new(format!("{}  Save Assistant", icons::SAVE)).min_size(eframe::egui::vec2(160.0, 18.0));
            if ui.add_enabled(!ed.busy, save_btn).clicked() {
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
                ui.spinner();
            }
        });
        if let Some((ok, msg)) = &ed.status {
            let color = if *ok { theme::success(ui) } else { theme::error(ui) };
            ui.label(RichText::new(msg).small().color(color));
        }
    });

    ui.add_space(10.0);
    ui.label(RichText::new(format!("{} My schedules", icons::SCHEDULE)).strong());
    if ed.schedules.is_empty() {
        ui.label(
            RichText::new("None. Ask the AI, e.g. \"remind me every Monday at 10 to count thermal paste\".")
                .small()
                .weak(),
        );
        return;
    }
    let mut cancel_id = None;
    for s in &ed.schedules {
        ui.horizontal_wrapped(|ui| {
            let next = s.next_run.map(|t| local_label(t.into_inner())).unwrap_or_default();
            ui.label(RichText::new(&s.title).strong());
            ui.label(RichText::new(format!("{} · next {next}", s.describe())).small().weak());
            if ui.small_button(format!("{} Cancel", icons::TRASH)).on_hover_text(s.id.key_string()).clicked() {
                cancel_id = Some(s.id.clone());
            }
        });
    }
    if let Some(id) = cancel_id {
        ed.busy = true;
        cancel(user_id.clone(), id);
    }
}
