use eframe::egui::{Align, Button, Color32, ComboBox, FontId, Id, Response, RichText, Stroke, TextEdit, Ui, Vec2, Widget};
use crate::utilities::{displays::tasks::task_cards::date_colors, Interaction, Updatable};
use database::schema::{Priority, Status, TaskPayload, TicketPayload, User};
use chrono::{DateTime, NaiveDate, Utc, Datelike};
use egui_extras::DatePickerButton;
use log::info;

impl Interaction for TaskPayload {
    fn interact_task_name(&mut self, ui: &mut Ui) -> Response {
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12,12,14);
        ui.style_mut().override_font_id = Some(FontId::proportional(12.0));
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(0.5, Color32::from_additive_luminance(110));
        let text_edit = TextEdit::singleline(&mut self.task_name).desired_width(ui.available_width() - 10.0).horizontal_align(Align::Center).vertical_align(Align::Center).ui(ui);

        if text_edit.lost_focus(){
            self.update_task_name(self.task_name.clone());
        }

        text_edit
    }

    fn interact_checkin_notes(&mut self, ui: &mut Ui) -> Response {
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12,12,14);
        let default = &mut TicketPayload::default();
        let ticket = self.service_ticket.as_mut().unwrap_or(default);
        let text_edit = TextEdit::multiline(&mut ticket.checkin_notes)
            .desired_rows(5)
            .desired_width(ui.available_width())
            .horizontal_align(Align::Center)
            .ui(ui);

        if text_edit.lost_focus() {
            let notes = ticket.clone().checkin_notes;
            self.update_checkin_notes(Some(notes));
            info!("checkin_notes changed: {:?}// {:?}", self.id, self.task_name);
        }
        text_edit
    }

    fn interact_task_description(&mut self, ui: &mut Ui) -> Response {
        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12,12,14);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));

        let text_edit = TextEdit::multiline(&mut self.task_description)
            .desired_rows(6)
            .desired_width(ui.available_width())
            .horizontal_align(Align::Center)
            .ui(ui);

        if text_edit.lost_focus() {
            self.update_task_description(self.task_description.clone());
        }

        text_edit
    }

    fn interact_due_date(&mut self, ui: &mut Ui) -> Response {
        let frame_color = date_colors(self.due_date.clone(), self.completed);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(0.5, frame_color);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(0.5, frame_color);
        let mut due_date = self.due_date.parse::<DateTime<Utc>>().unwrap().date_naive();
        
        let id = self.id.clone().unwrap().0.id.to_string();
        let date_picker = DatePickerButton::new(&mut due_date)
            .format("%m/%d/%y")
            .id_source(id.as_str())
            .show_icon(false)
            .ui(ui);

        if date_picker.changed(){
            // Combine the NaiveDate with a default time to create a DateTime<Utc>
            let date_time = NaiveDate::from_ymd_opt(due_date.year(), due_date.month(), due_date.day())
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Utc)
                .unwrap();

            let rfc3339_date = date_time.to_rfc3339();
            let date = due_date.clone().to_string();
            self.update_due_date(rfc3339_date.clone());
            info!("date_widget changed: {:?}// {:?} ", self.task_name,  date);
        }

        date_picker
    }

    fn interact_completed(&mut self, ui: &mut Ui) -> Response {
        if self.completed{
            let hover_txt = "✔";
            let color_complete = Color32::LIGHT_GREEN;
            let stroke = Stroke::new(1.0, color_complete);
            return Button::new(hover_txt).stroke(stroke).small().min_size(Vec2::new(25.0, 20.0)).ui(ui);
        }else{
            let hover_txt = "✖";
            let color_incomplete = Color32::LIGHT_RED;
            let stroke = Stroke::new(1.0, color_incomplete);
            return Button::new(hover_txt).stroke(stroke).small().min_size(Vec2::new(25.0, 20.0)).ui(ui);
        }
    }

    fn interact_status(&mut self, ui: &mut Ui) -> Response {
        ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(RichText::new(format!("{}", &self.status.as_str())))
            .width(ui.available_width() - 15.0)
            .height(ui.available_height())
            .show_ui(ui, |ui| 
        {
            for status in Status::VALUES{
                let status_change = ui.selectable_value(&mut self.status, status.to_owned(), status.as_str());
                if status_change.clicked(){
                    self.update_status(status.clone());
                }
            }
        }).response
    }

    fn interact_priority(&mut self, ui: &mut Ui) -> Response {
        ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(RichText::new(format!("{}", &self.priority.as_str())))
            .width(ui.available_width() - 15.0)
            .height(ui.available_height() - 2.0)
            .show_ui(ui, |ui| 
        {
            for priority in Priority::VALUES{
                let priority_change = ui.selectable_value(&mut self.priority, priority.to_owned(), priority.as_str());
                if priority_change.clicked(){
                    self.update_priority(Some(priority.clone()));
                }
            }
        }).response
    }

    fn interact_assignee_initials(&mut self, ui: &mut Ui, store_users: &Vec<User>) -> Response {
        ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(RichText::new(&self.everest_initials).small())
            .width(ui.available_width() / 1.3)
            .height(ui.available_height() - 2.0)
            .show_ui(ui, |ui| 
        {
            for user in *&store_users{
                let assignee_selection = ui.selectable_value(&mut self.everest_initials, user.everest_initials.to_owned(), &user.everest_initials);
                if assignee_selection.clicked(){
                    self.update_assignee_initials(user.everest_initials.clone());
                }
            }
        }).response
    }
}