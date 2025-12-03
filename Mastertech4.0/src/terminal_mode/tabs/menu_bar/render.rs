use database::schema::User;
use ratatui::{crossterm::event::MouseEvent, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::Stylize, widgets::{Block, Paragraph, Widget, WidgetRef, Wrap}, Frame};

use crate::{filesystem::get_client_hash, terminal_mode::{styling::CATPPUCCIN, widgets::{ButtonType, HandleWidget, ShrinkArea}}};

use super::MenuBar;


impl <'a> HandleWidget <'_> for MenuBar <'_> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        // let tab_order = self.tabs.len();
        // let mut constraints = vec![Constraint::Length(20); tab_order];
        // constraints.push(Constraint::Length(25)); // For the title paragraph
        // let row = Layout::default()
        //     .direction(Direction::Horizontal)
        //     .constraints(constraints)
        //     .split(area);
        // for (idx, (_, button)) in self.tabs.iter().enumerate() {
        //     button.render_ref(row[idx].shrink(3, 1), f.buffer_mut());
        // }

        let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20), Constraint::Length(20),
            Constraint::Length(20), Constraint::Length(20),
            Constraint::Length(20), Constraint::Length(20),
            Constraint::Length(20), Constraint::Length(20),
            Constraint::Length(20), Constraint::Length(25),
        ])
        .split(area);

        self.ticket_tab.render_ref(row[0].shrink(3, 1), f.buffer_mut());
        self.scripts_tab.render_ref(row[1].shrink(3, 1), f.buffer_mut());
        self.system_tab.render_ref(row[2].shrink(3, 1), f.buffer_mut());
        self.ncdu_tab.render_ref(row[3].shrink(3, 1), f.buffer_mut());
        self.tasks_tab.render_ref(row[4].shrink(3, 1), f.buffer_mut());
        self.webconsole_tab.render_ref(row[5].shrink(3, 1), f.buffer_mut());
        self.logs_tab.render_ref(row[6].shrink(3, 1), f.buffer_mut());
        self.login_tab.render_ref(row[7].shrink(3, 1), f.buffer_mut());
        let title = &mut self.client_title;
        let user = &mut User::default();

        if user.get_name().is_empty() {
            if let Ok(ctx) = self.ctx.lock() {
                if !ctx.user.get_name().is_empty() {
                    *user = ctx.user.clone();
                }
            }
        }

        if title.is_empty() {
            let client = get_client_hash();
            *title = client.connection_string.clone();
        } else {
            let para = Paragraph::new(format!("{}", &**title))
                .block(
                    Block::default()
                        .title_alignment(ratatui::layout::Alignment::Center)
                        .border_type(ratatui::widgets::BorderType::Rounded)
                        .fg(CATPPUCCIN.lavender)
                        .title(user.get_name())
                )
                .right_aligned()
                .wrap(Wrap{ trim: false});
            (&para).render(row[8], f.buffer_mut());
        }

        let state = &self.connection_state;
        let color = if state.0 { CATPPUCCIN.green } else { CATPPUCCIN.maroon };
        if state.0 {
            let para = Paragraph::new(format!("Server Msg: {}", state.1))
            .block(
                Block::default()
                    .title_alignment(ratatui::layout::Alignment::Center)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .fg(color)
                    .title(format!("Connected: {}", state.0))
            )
            .right_aligned()
            .wrap(Wrap{ trim: false});
            (&para).render(row[9], f.buffer_mut());
        } else {
            self.connect_ws_btn.render_ref(row[9].shrink(3, 1), f.buffer_mut());
        }

        // self.effect_stage.process_effects(
        //     tachyonfx::Duration::from_millis(16), 
        //     f.buffer_mut(), 
        //     area
        // );
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.ticket_tab.handle_mouse_event(&mouse_event);
        self.scripts_tab.handle_mouse_event(&mouse_event);
        self.tasks_tab.handle_mouse_event(&mouse_event);
        self.system_tab.handle_mouse_event(&mouse_event);
        self.ncdu_tab.handle_mouse_event(&mouse_event);
        self.logs_tab.handle_mouse_event(&mouse_event);
        self.login_tab.handle_mouse_event(&mouse_event);
        self.webconsole_tab.handle_mouse_event(&mouse_event);
        self.connect_ws_btn.handle_mouse_event(&mouse_event);
    }
}
