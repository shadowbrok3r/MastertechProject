
use eframe::egui::Ui;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use crate::app_state::MtechServerContext;
use self::chart::render_chart1;

pub mod chart;

impl MtechServerContext {
    pub fn terminal(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();
        
        self.terminal
            .draw(|frame| {
                let area = frame.size();
                let app = &self.chart_app;
                render_chart1(frame, area, &app);
            })
        .expect("epic fail");
      // ui.add( self.terminal.backend_mut());
    }
}


fn _centered_rect(r: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let popup_layout = Layout::default()
      .direction(Direction::Vertical)
      .constraints([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
      ])
      .split(r);
  
    Layout::default()
      .direction(Direction::Horizontal)
      .constraints([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
      ])
      .split(popup_layout[1])[1]
  }