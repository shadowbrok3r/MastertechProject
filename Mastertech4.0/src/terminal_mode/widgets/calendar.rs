use chrono::{Date, Days, Local, Months};
use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer, layout::Rect, style::{Color, Style, Stylize}, text::Span, widgets::{
        calendar::{CalendarEventStore, Monthly}, Widget
    }
};
use std::{collections::HashMap, iter};
use std::sync::atomic::Ordering;

/*
 * https://github.com/TadoTheMiner/kraban
*/

#[derive(Debug, Clone, Copy)]
pub struct DueDatePrompt {
    old_date: Option<time::Date>,
    current_date: ChronoDate,
}

impl DueDatePrompt {
    pub fn new(old_date: Option<time::Date>) -> Self {
        let current_date = old_date
            .map(time_date_to_chrono_date)
            .unwrap_or(Local::now());
        Self {
            old_date,
            current_date,
        }
    }
}

impl PromptTrait for DueDatePrompt {
    fn height(&self) -> u16 {
        8 // Calendar has max 5 rows + month header + weekdays header
    }

    fn width(&self) -> u16 {
        22 // Each column has 2 characters with a space in between. Plus we add 2 for spacing
    }

    fn title(&self, _item: Item) -> String {
        "Change due date".to_string()
    }
}

const DAYS_IN_WEEK: u64 = 7;
impl Component for DueDatePrompt {
    fn on_key(&mut self, key_event: KeyEvent, _context: Context) -> Option<Action> {
        self.current_date = match key_event.code {
            KeyCode::Tab => self.current_date.checked_add_months(Months::new(1)),
            KeyCode::BackTab => self.current_date.checked_sub_months(Months::new(1)),
            KeyCode::Right => self.current_date.checked_add_days(Days::new(1)),
            KeyCode::Left => self.current_date.checked_sub_days(Days::new(1)),
            KeyCode::Up => self.current_date.checked_sub_days(Days::new(DAYS_IN_WEEK)),
            KeyCode::Down => self.current_date.checked_add_days(Days::new(DAYS_IN_WEEK)),
            KeyCode::Enter => {
                return state_action(StateAction::SetTaskDueDate(Some(chrono_date_to_time_date(
                    self.current_date,
                ))));
            }
            KeyCode::Delete | KeyCode::Backspace => {
                return state_action(StateAction::SetTaskDueDate(None));
            }
            _ => None,
        }?;
        None
    }

    // fn key_hints(&self, _context: Context) -> KeyHints {
    //     vec![
    //         ("Delete/Backspace", "Delete due date"),
    //         ("Tab/Backtab", "Switch month"),
    //         ("Arrows", "Pick day"),
    //         ("Enter", "Submit"),
    //     ]
    // }

    fn render(&self, area: Rect, buf: &mut Buffer, context: Context) {
        let selected_date = chrono_date_to_time_date(self.current_date);
        let selected_style = Style::new().fg(context.config.app_color).reversed();
        let today = chrono_date_to_time_date(Local::now());
        let today_style = Style::new().fg(Color::Green).reversed();
        let old_date_style = Style::new().fg(Color::Yellow).reversed();
        let event_store = CalendarEventStore(HashMap::from_iter(
            iter::once((today, today_style))
                .chain(self.old_date.map(|old_date| (old_date, old_date_style)))
                .chain(iter::once((selected_date, selected_style))),
        ));

        Monthly::new(selected_date, event_store)
            .show_surrounding(Style::new().fg(Color::DarkGray))
            .show_weekdays_header(Style::new().fg(Color::Green).italic())
            .show_month_header(Style::new().fg(Color::Yellow).bold())
            .render(area, buf);
    }
}

pub fn date_to_span<Tz>(date: Date<Tz>) -> Span<'static> {
    let duration = date - OffsetDateTime::now_local()
        .inspect(|e| log::error!("Failed to get local timezone using utc {e}"))
        .unwrap_or(OffsetDateTime::now_utc())
        .date();

    let color = match duration.whole_days() {
        ..0 => Color::Red,
        0 => Color::Yellow,
        1..7 => Color::Green,
        7..30 => Color::Blue,
        _ => Color::Magenta,
    };

    date.to_string().fg(color).underlined()
}



pub fn compare_due_dates(first: Option<Date>, second: Option<Date>) -> Ordering {
    match (first, second) {
        (Some(first), Some(second)) => second.cmp(&first),
        _ => first.cmp(&second),
    }
}

pub type ChronoDate = chrono::DateTime<Local>;
// hate that there's two crates which do not fully implement my use case but whathever
pub fn chrono_date_to_time_date(chrono_date: ChronoDate) -> time::Date {
    let year = chrono_date.year();
    let month = time::Month::December.nth_next(chrono_date.month() as u8);
    let day = chrono_date.day();
    time::Date::from_calendar_date(year, month, day as u8).unwrap()
}

pub fn time_date_to_chrono_date(time_date: time::Date) -> ChronoDate {
    let year = time_date.year();
    let month = time_date.month() as u32;
    let day = time_date.day() as u32;
    ChronoDate::default()
        .with_year(year)
        .unwrap()
        .with_month(month)
        .unwrap()
        .with_day(day)
        .unwrap()
}