//! Full-width selectable list row with a truncated title, an optional detail line and an unread dot.

use eframe::egui::{
    Align2, Color32, FontId, Rect, Response, RichText, Sense, Spinner, TextStyle, TextWrapMode, Ui,
    WidgetText, pos2, vec2,
};

use crate::ui_tools::theme;

/// Width reserved for the leading icon or spinner.
const LEAD_W: f32 = 18.0;
/// Width reserved for the unread dot.
const DOT_W: f32 = 12.0;
/// Width reserved for the action button.
const ACTION_W: f32 = 20.0;
const DOT_RADIUS: f32 = 3.5;
const BAR_H: f32 = 3.0;
/// Gap between the text and the progress bar.
const BAR_GAP: f32 = 3.0;

/// What sits in front of a row's title.
#[derive(Clone, Copy, Debug)]
pub enum Lead<'a> {
    None,
    Icon(&'a str, Option<Color32>),
    Spinner(Option<Color32>),
}

/// One row of a session or automation list.
#[derive(Clone, Copy, Debug)]
pub struct ListRow<'a> {
    pub lead: Lead<'a>,
    pub title: &'a str,
    pub detail: Option<&'a str>,
    pub selected: bool,
    pub unread: bool,
    /// Icon and tooltip of a right-aligned button shown while the row is hovered or selected.
    pub action: Option<(&'a str, &'a str)>,
    /// Share done, 0.0 to 1.0, drawn as a thin bar under the text.
    pub progress: Option<f32>,
}

impl<'a> ListRow<'a> {
    pub fn new(title: &'a str) -> Self {
        Self { lead: Lead::None, title, detail: None, selected: false, unread: false, action: None, progress: None }
    }

    pub fn progress(mut self, share: f32) -> Self {
        self.progress = Some(share.clamp(0.0, 1.0));
        self
    }

    pub fn lead(mut self, lead: Lead<'a>) -> Self {
        self.lead = lead;
        self
    }

    pub fn detail(mut self, detail: &'a str) -> Self {
        self.detail = Some(detail);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn unread(mut self, unread: bool) -> Self {
        self.unread = unread;
        self
    }

    pub fn action(mut self, icon: &'a str, tip: &'a str) -> Self {
        self.action = Some((icon, tip));
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        self.show_with_action(ui).0
    }

    /// Draws the row; the second response is the action button's while it shows.
    pub fn show_with_action(self, ui: &mut Ui) -> (Response, Option<Response>) {
        let pad = ui.spacing().button_padding;
        let action_w = if self.action.is_some() { ACTION_W } else { 0.0 };
        let width = ui.available_width().max(LEAD_W + DOT_W + action_w + 2.0 * pad.x + 24.0);
        let lead_w = if matches!(self.lead, Lead::None) { 0.0 } else { LEAD_W };
        let text_w = width - 2.0 * pad.x - lead_w - DOT_W - action_w;

        let title = if self.unread { RichText::new(self.title).strong() } else { RichText::new(self.title) };
        let title = WidgetText::from(title).into_galley(ui, Some(TextWrapMode::Truncate), text_w, TextStyle::Body);
        let detail = self.detail.map(|d| {
            WidgetText::from(RichText::new(d).small().color(theme::weak_text(ui)))
                .into_galley(ui, Some(TextWrapMode::Truncate), text_w, TextStyle::Small)
        });
        let title_h = title.size().y;
        let text_h = title_h + detail.as_ref().map_or(0.0, |g| g.size().y + 1.0);
        let bar_h = if self.progress.is_some() { BAR_GAP + BAR_H } else { 0.0 };
        let height = text_h + bar_h + 2.0 * pad.y;

        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        let inner = rect.shrink2(pad);
        let action = self.action.filter(|_| self.selected || response.contains_pointer()).map(|(icon, tip)| {
            let side = ACTION_W.min(inner.height());
            let spot = Rect::from_center_size(pos2(inner.max.x - DOT_W - ACTION_W / 2.0, inner.center().y), vec2(side, side));
            (icon, ui.interact(spot, response.id.with("action"), Sense::click()).on_hover_text(tip))
        });
        if !ui.is_rect_visible(rect) {
            return (response, action.map(|(_, button)| button));
        }
        let visuals = ui.style().interact_selectable(&response, self.selected);
        let button_hovered = action.as_ref().is_some_and(|(_, button)| button.hovered());
        if self.selected || response.hovered() || button_hovered || response.highlighted() || response.has_focus() {
            ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        }

        let lead_rect = Rect::from_min_size(inner.min, vec2(lead_w, title_h));
        match self.lead {
            Lead::None => {}
            Lead::Icon(icon, color) => {
                ui.painter().text(
                    lead_rect.left_center(),
                    Align2::LEFT_CENTER,
                    icon,
                    FontId::proportional(title_h * 0.8),
                    color.unwrap_or_else(|| visuals.text_color()),
                );
            }
            Lead::Spinner(color) => {
                let side = (title_h - 2.0).max(8.0);
                let square = Rect::from_min_size(pos2(lead_rect.min.x, lead_rect.center().y - side / 2.0), vec2(side, side));
                let mut spinner = Spinner::new().size(side);
                if let Some(c) = color {
                    spinner = spinner.color(c);
                }
                spinner.paint_at(ui, square);
            }
        }

        let text_x = inner.min.x + lead_w;
        ui.painter().galley(pos2(text_x, inner.min.y), title, visuals.text_color());
        if let Some(detail) = detail {
            ui.painter().galley(pos2(text_x, inner.min.y + title_h + 1.0), detail, theme::weak_text(ui));
        }
        if let Some(share) = self.progress {
            let track = Rect::from_min_size(pos2(text_x, inner.min.y + text_h + BAR_GAP), vec2(text_w, BAR_H));
            let radius = BAR_H / 2.0;
            ui.painter().rect_filled(track, radius, theme::faint_text(ui).gamma_multiply(0.35));
            if share > 0.0 {
                let fill = if share >= 1.0 { theme::success(ui) } else { theme::accent(ui) };
                let done = Rect::from_min_size(track.min, vec2(track.width() * share, BAR_H));
                ui.painter().rect_filled(done, radius, fill);
            }
        }
        if self.unread {
            let center = pos2(inner.max.x - DOT_W / 2.0, inner.min.y + title_h / 2.0);
            ui.painter().circle_filled(center, DOT_RADIUS, theme::accent(ui));
        }
        if let Some((icon, button)) = &action {
            let color = if button.hovered() {
                ui.painter().rect_filled(button.rect, visuals.corner_radius, ui.visuals().widgets.hovered.weak_bg_fill);
                visuals.text_color()
            } else {
                theme::weak_text(ui)
            };
            ui.painter().text(button.rect.center(), Align2::CENTER_CENTER, *icon, FontId::proportional(title_h * 0.8), color);
        }
        (response, action.map(|(_, button)| button))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput};

    fn run(width: f32, mut f: impl FnMut(&mut Ui)) {
        let ctx = Context::default();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 400.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            CentralPanel::default().show(ui, |ui| f(ui));
        });
        out.textures_delta.clear();
    }

    #[test]
    fn a_long_title_keeps_the_row_inside_the_available_width() {
        let long = "webhook_27517d89-fe4a-439e-a02e-8ec9c4d9cecd_27517d89-fe4a-439e-a02e-8ec9c4d9cecd";
        let mut rects = Vec::new();
        run(240.0, |ui| {
            let available = ui.available_width();
            let row = ListRow::new(long).lead(Lead::Icon("x", None)).detail(long).unread(true).show(ui);
            rects.push((available, row.rect.width()));
        });
        let (available, width) = rects[0];
        assert!((width - available).abs() < 0.5, "row {width} vs available {available}");
    }

    #[test]
    fn a_progress_bar_adds_its_height_below_the_text() {
        let mut heights = Vec::new();
        run(240.0, |ui| {
            let plain = ListRow::new("Martin Empey - 2141021").detail("detail").show(ui);
            let barred = ListRow::new("Martin Empey - 2141021").detail("detail").progress(0.4).show(ui);
            heights.push((plain.rect.height(), barred.rect.height()));
        });
        let (plain, barred) = heights[0];
        assert!((barred - plain - BAR_GAP - BAR_H).abs() < 0.5, "plain {plain} vs barred {barred}");
        assert_eq!(ListRow::new("x").progress(1.7).progress, Some(1.0));
    }

    /// Runs one frame per event batch and returns what `f` recorded on the last one.
    fn frames<T>(batches: Vec<Vec<Event>>, mut f: impl FnMut(&mut Ui) -> T) -> T {
        let ctx = Context::default();
        let mut last = None;
        for events in batches {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 400.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                CentralPanel::default().show(ui, |ui| last = Some(f(ui)));
            });
            out.textures_delta.clear();
        }
        last.expect("a frame ran")
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// Hovers the row, then clicks at the point `at` picks from the row and action rects.
    fn click(at: impl Fn(Rect, Rect) -> Pos2) -> (bool, bool) {
        let row = |ui: &mut Ui| {
            let (row, action) = ListRow::new("session").action("x", "Archive").show_with_action(ui);
            (row.rect, action.as_ref().map(|a| a.rect), row.clicked(), action.is_some_and(|a| a.clicked()))
        };
        let (row_rect, _, _, _) = frames(vec![vec![]], row);
        let hover = vec![Event::PointerMoved(row_rect.center())];
        let (_, action_rect, _, _) = frames(vec![hover.clone(), hover.clone()], row);
        let target = at(row_rect, action_rect.expect("the action shows while hovered"));
        let (_, _, row_clicked, action_clicked) = frames(
            vec![
                hover.clone(),
                hover,
                vec![Event::PointerMoved(target)],
                vec![button(target, true)],
                vec![button(target, false)],
            ],
            row,
        );
        (row_clicked, action_clicked)
    }

    #[test]
    fn the_action_button_takes_its_own_click() {
        assert_eq!(click(|_, action| action.center()), (false, true), "a click on the button");
        assert_eq!(click(|row, _| row.left_center() + vec2(30.0, 0.0)), (true, false), "a click on the title");
    }

    #[test]
    fn the_action_button_hides_until_the_row_is_hovered() {
        let action = frames(vec![vec![]], |ui| ListRow::new("session").action("x", "Archive").show_with_action(ui).1);
        assert!(action.is_none());
        let selected = frames(vec![vec![]], |ui| {
            ListRow::new("session").action("x", "Archive").selected(true).show_with_action(ui).1
        });
        assert!(selected.is_some(), "a selected row always shows it");
    }

    #[test]
    fn a_short_title_still_fills_the_width() {
        let mut widths = Vec::new();
        run(300.0, |ui| {
            let available = ui.available_width();
            widths.push((available, ListRow::new("hello").show(ui).rect.width()));
        });
        let (available, width) = widths[0];
        assert!((width - available).abs() < 0.5, "row {width} vs available {available}");
    }
}
