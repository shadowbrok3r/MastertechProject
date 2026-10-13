//! Session and automation list rows: a [`ListItem`] with a status lead, detail line and unread dot.

use eframe::egui::{Color32, Response, Ui};

use crate::ui_tools::controls::ListItem;
use crate::ui_tools::icons::svg::Icon;

/// What sits in front of a row's title.
#[derive(Clone, Copy, Debug)]
pub enum Lead {
    None,
    Icon(Icon, Option<Color32>),
    Spinner(Option<Color32>),
}

/// One row of a session or automation list.
#[derive(Clone, Copy, Debug)]
pub struct ListRow<'a> {
    pub lead: Lead,
    pub title: &'a str,
    pub detail: Option<&'a str>,
    pub selected: bool,
    pub unread: bool,
    /// Icon and tooltip of a right-aligned button shown while the row is hovered or selected.
    pub action: Option<(Icon, &'a str)>,
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

    pub fn lead(mut self, lead: Lead) -> Self {
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

    pub fn action(mut self, icon: Icon, tip: &'a str) -> Self {
        self.action = Some((icon, tip));
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        self.show_with_action(ui).0
    }

    /// Draws the row; the second response is the action button's while it shows.
    pub fn show_with_action(self, ui: &mut Ui) -> (Response, Option<Response>) {
        let mut item = ListItem::new(self.title)
            .subtitle(self.detail.unwrap_or_default())
            .selected(self.selected)
            .unread(self.unread);
        match self.lead {
            Lead::None => {}
            Lead::Icon(icon, tone) => {
                item = item.icon(icon);
                if let Some(tone) = tone {
                    item = item.icon_tone(tone);
                }
            }
            Lead::Spinner(tone) => item = item.spinner(tone),
        }
        if let Some((icon, tip)) = self.action {
            item = item.action(icon, tip);
        }
        if let Some(share) = self.progress {
            item = item.progress(share);
        }
        item.show_with_action(ui)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_tools::icons::svg;
    use eframe::egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

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
            let row = ListRow::new(long).lead(Lead::Icon(svg::X, None)).detail(long).unread(true).show(ui);
            rects.push((available, row.rect.width()));
        });
        let (available, width) = rects[0];
        assert!((width - available).abs() < 0.5, "row {width} vs available {available}");
    }

    #[test]
    fn a_progress_bar_makes_the_row_taller() {
        let mut heights = Vec::new();
        run(240.0, |ui| {
            let plain = ListRow::new("Ticket 1000001").detail("detail").show(ui);
            let barred = ListRow::new("Ticket 1000001").detail("detail").progress(0.4).show(ui);
            heights.push((plain.rect.height(), barred.rect.height()));
        });
        let (plain, barred) = heights[0];
        assert!(barred > plain, "plain {plain} vs barred {barred}");
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
            let (row, action) = ListRow::new("session").action(svg::ARCHIVE, "Archive").show_with_action(ui);
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
        let action = frames(vec![vec![]], |ui| ListRow::new("session").action(svg::ARCHIVE, "Archive").show_with_action(ui).1);
        assert!(action.is_none());
        let selected = frames(vec![vec![]], |ui| {
            ListRow::new("session").action(svg::ARCHIVE, "Archive").selected(true).show_with_action(ui).1
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
