use ratatui::{layout::{Alignment, Constraint, Direction, Layout, Rect}, prelude::{Backend, Color}, style::{Modifier, Style}, symbols::line::DOUBLE, widgets::{Block, BorderType, Borders, Clear, LineGauge, Paragraph, Wrap}, Frame};
use crate::terminal_mode::{fx::{effect::UniqueEffectId, EffectStage}, styling::THEME};
use std::{hash::Hasher, time::Instant};

#[allow(dead_code)]
#[derive(Clone, PartialEq, Debug)]
// Notification types
pub enum NotificationType {
    Info,
    Warning,
    Error,
    Other,
}

#[derive(Clone, Debug)]
// Notification struct
pub struct Notification {
    pub notification_type: NotificationType,
    pub header: String,
    pub text: String,
    pub duration_secs: u64,
    pub created_at: Instant, // track creation time
}

impl Notification {
    pub fn new(notification_type: NotificationType, header: &str, text: &str, duration_secs: u64) -> Self {
        Self {
            notification_type,
            header: header.into(),
            text: text.into(),
            duration_secs,
            created_at: Instant::now(),
        }
    }

    pub fn id(&self) -> usize {
        let hasher = &mut std::collections::hash_map::DefaultHasher::new();
        hasher.write(
            format!("{} {}", self.text, self.header).as_bytes()
        );
        // simple way: hash header and text
        std::hash::Hasher::finish(hasher) as usize
    }
    
    pub fn elapsed_ratio(&self) -> f64 {
        let elapsed = Instant::now().duration_since(self.created_at);
        let ratio = (elapsed.as_secs_f64() / self.duration_secs as f64).clamp(0.0, 1.0);
        ratio
    }

    pub fn notification_area<B: Backend>(&self, frame: &Frame) -> Rect {
        let popup_width = 70;
        
        // Estimate height based on text length (assuming ~70 chars per line)
        let text_lines = (self.text.len() as f64 / popup_width as f64).ceil() as u16;
        let popup_height = text_lines + 6; // additional space for header, padding, and gauge
    
        Rect {
            x: frame.area().width.saturating_sub(popup_width),
            y: 0,
            width: popup_width.min(frame.area().width),
            height: popup_height.min(frame.area().height),
        }
    }

    pub fn is_expired(&self) -> bool {
        let expiry = self.created_at.elapsed().as_secs() >= self.duration_secs;
        expiry
    }

    pub fn border_color(&self) -> Color {
        match self.notification_type {
            NotificationType::Info => THEME.accent,
            NotificationType::Warning => THEME.warning,
            NotificationType::Error => THEME.error,
            NotificationType::Other => THEME.tertiary,
        }
    }

    #[allow(dead_code)]
    pub fn render_effects(&self, _effect_stage: &mut EffectStage<UniqueEffectId>, _area: Rect) {
        // let border_color = self.border_color();
        // let effect = outline_selected_cells(
        //     effect_stage, 
        //     area.as_size(),
        //     border_color,
        //     CellFilter::All // FgColor(border_color)
        // );
        // effect_stage.add_effect(effect);
    }

    pub fn display<B: Backend>(&self, frame: &mut Frame<'_>) {
        let area = self.notification_area::<B>(frame);
        let border_color = self.border_color();
        frame.render_widget(Clear, area);

        // Create a centered block as the container
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(self.header.as_str())
            .title_alignment(Alignment::Center)
            .style(Style::default().add_modifier(Modifier::BOLD).fg(border_color));
 
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1), // Text (dynamic height) 
                Constraint::Length(2), // Spacer
                Constraint::Length(1), // Gauge
            ])
            .split(area);

        let text = Paragraph::new(format!("  {}", self.text.as_str()))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false });

        // The gauge ratio is always full in sync render; timer countdown is async-managed.
        let gauge = LineGauge::default()
            // .block(
            //     Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(border_color))
            // )
            .filled_symbol(DOUBLE.horizontal)
            .style(Style::default().fg(THEME.surface))
            .filled_style(Style::default().fg(THEME.accent))
            .ratio(self.elapsed_ratio());

        frame.render_widget(text, chunks[0]);
        frame.render_widget(gauge, chunks[2]);
    }
}