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
const DOT_RADIUS: f32 = 3.5;

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
}

impl<'a> ListRow<'a> {
    pub fn new(title: &'a str) -> Self {
        Self { lead: Lead::None, title, detail: None, selected: false, unread: false }
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

    pub fn show(self, ui: &mut Ui) -> Response {
        let pad = ui.spacing().button_padding;
        let width = ui.available_width().max(LEAD_W + DOT_W + 2.0 * pad.x + 24.0);
        let lead_w = if matches!(self.lead, Lead::None) { 0.0 } else { LEAD_W };
        let text_w = width - 2.0 * pad.x - lead_w - DOT_W;

        let title = if self.unread { RichText::new(self.title).strong() } else { RichText::new(self.title) };
        let title = WidgetText::from(title).into_galley(ui, Some(TextWrapMode::Truncate), text_w, TextStyle::Body);
        let detail = self.detail.map(|d| {
            WidgetText::from(RichText::new(d).small().color(theme::weak_text(ui)))
                .into_galley(ui, Some(TextWrapMode::Truncate), text_w, TextStyle::Small)
        });
        let title_h = title.size().y;
        let height = title_h + detail.as_ref().map_or(0.0, |g| g.size().y + 1.0) + 2.0 * pad.y;

        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let visuals = ui.style().interact_selectable(&response, self.selected);
        if self.selected || response.hovered() || response.highlighted() || response.has_focus() {
            ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        }

        let inner = rect.shrink2(pad);
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
        if self.unread {
            let center = pos2(inner.max.x - DOT_W / 2.0, inner.min.y + title_h / 2.0);
            ui.painter().circle_filled(center, DOT_RADIUS, theme::accent(ui));
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{CentralPanel, Context, RawInput};

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
