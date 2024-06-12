
use egui::Ui;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use websocket::TerminalFrontend;
use crate::app_state::MtechServerContext;
use self::chart::render_chart1;

pub mod chart;
pub mod websocket;

impl MtechServerContext {
    pub fn terminal(&mut self, ui: &mut Ui){
        let ctx = ui.ctx().clone();
        ctx.request_repaint();
        let wakeup = move || ctx.request_repaint();
        self.terminal
            .draw(|frame| {
                let app = &self.chart_app;
                let area = frame.size();
                // render_chart1(frame, area, &app);
        
                if let Some(frontend) = &mut self.terminal_frontend {
                  frontend.ui(ui, frame, area);
                }else{
                    render_chart1(frame, area, &app);
                }
            })
        .expect("epic fail");

        ui.add( self.terminal.backend_mut());

        if ui.button("Connect").clicked()
        {
            match ewebsock::connect_with_wakeup(&self.url, Default::default(), wakeup) {
                Ok((ws_sender, ws_receiver)) => {
                    self.terminal_frontend = Some(TerminalFrontend::new(ws_sender, ws_receiver));
                    self.error.clear();
                }
                Err(error) => {
                    log::error!("Failed to connect to {:?}: {}", &self.url, error);
                    self.error = error;
                }
            };
        }

        if !self.error.is_empty() {
            egui::TopBottomPanel::top("error").show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Error:");
                    ui.colored_label(egui::Color32::RED, &self.error);
                });
            });
        }
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