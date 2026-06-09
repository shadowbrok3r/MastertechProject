use database::schema::User;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    prelude::Backend,
    style::Style,
    widgets::{Block, BorderType, Paragraph, Widget, WidgetRef, Wrap},
    Frame,
};

use crate::{
    filesystem::get_client_hash,
    terminal_mode::{
        styling::THEME,
        widgets::{HandleWidget, ShrinkArea},
    },
};

use super::MenuBar;

impl<'a> MenuBar<'a> {
    /// Renders the open dropdown over the full frame. Called after the content
    /// tab so the menu paints on top.
    pub fn draw_overlay(&mut self, f: &mut Frame) {
        let frame = f.area();
        self.dropdown.borrow_mut().render(f, frame);
    }
}

impl<'a> HandleWidget<'_> for MenuBar<'_> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let cols = Layout::horizontal([
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Fill(1),
            Constraint::Length(24),
        ])
        .split(area);

        self.service_trigger.render_ref(cols[0].shrink_symmetric(1, 1), f.buffer_mut());
        self.tools_trigger.render_ref(cols[1].shrink_symmetric(1, 1), f.buffer_mut());
        self.remote_trigger.render_ref(cols[2].shrink_symmetric(1, 1), f.buffer_mut());
        self.account_trigger.render_ref(cols[3].shrink_symmetric(1, 1), f.buffer_mut());

        let title = &mut self.client_title;
        let user = &mut User::default();

        if let Ok(ctx) = self.ctx.lock() {
            if !ctx.user.get_name().is_empty() {
                *user = ctx.user.clone();
            }
            if let Some(name) = &ctx.friendly_name {
                *title = name.clone();
            }
        }

        if title.is_empty() {
            *title = get_client_hash().connection_string.clone();
        }

        let connected = self.connection_state.0;
        let title_para = Paragraph::new(format!("{}", &**title))
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(THEME.border(connected))
                    .title(user.get_name())
                    .title_alignment(Alignment::Center),
            )
            .right_aligned()
            .wrap(Wrap { trim: false });
        (&title_para).render(cols[4].shrink(1, 1), f.buffer_mut());

        if connected {
            let para = Paragraph::new(format!("Server: {}", self.connection_state.1))
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style(Style::new().fg(THEME.success))
                        .title("Connected")
                        .title_alignment(Alignment::Center),
                )
                .right_aligned()
                .wrap(Wrap { trim: false });
            (&para).render(cols[5].shrink(1, 1), f.buffer_mut());
        } else {
            self.connect_ws_btn.render_ref(cols[5].shrink(1, 1), f.buffer_mut());
        }
    }
}
