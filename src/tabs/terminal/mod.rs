use crate::app_state::app_state::MtechServerContext;
use egui::{Sense, Ui, Widget, WidgetInfo};
use log::info;
use ratatui::{buffer::Buffer, layout::{Alignment, Constraint, Direction, Layout, Rect}, prelude::Stylize, symbols::border, widgets::{block::{Position, Title}, Block, Borders, Clear, Paragraph}};

impl MtechServerContext {
    pub fn terminal(&mut self, ui: &mut Ui){

        let mut buf = Buffer::empty(Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        });

        buf.get_mut(0, 2).set_symbol("x");
        
        self.terminal
            .draw(|frame| {
                let area = frame.size();
                println!("area: {area:?}");
                frame.set_cursor(0, 0);

                let popup_area = centered_rect(area, 35, 35);

                let block = Block::default()
                    .title(Title::from("Title").alignment(Alignment::Center))
                    .title(
                        Title::from("X")
                            .alignment(Alignment::Center)
                            .position(Position::Bottom),
                    )
                    .borders(Borders::ALL)
                    .border_set(border::THICK)
                    .white();

                let para = Paragraph::new("Hello Egui")
                    .centered()
                    .block(block)
                    .cyan()
                    .on_black();

                // frame.render_widget(Clear, popup_area);
                // frame.render_widget(Block::default().borders(Borders::all()).title("Main"), popup_area);
                frame.render_widget(para, area);
            })
        .expect("epic fail");

        let x =         ui.add(
            self.terminal.backend_mut()
        );
        
        x.paint_debug_info();
        if x.context_menu_opened(){
            println!("CONTEXT MENU OPENED");
        }if x.is_tooltip_open(){
            println!("TOOLTIP OPENED");
            
        }if x.hovered(){
            println!("HOVERED");
        }if x.is_pointer_button_down_on(){
            println!("POINTER BUTTON");
        }if x.sense.interactive(){
            println!("INTERACTIVE");
        }



        if ui.input(|i| i.key_released(egui::Key::Q)) {
            panic!("HAVE A NICE WEEK");
        }
        if ui.input(|i| i.key_released(egui::Key::T)) {
            ()
        }
    }
}


fn centered_rect(r: Rect, percent_x: u16, percent_y: u16) -> Rect {
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