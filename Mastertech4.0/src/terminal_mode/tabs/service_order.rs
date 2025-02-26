use crossbeam::channel::Sender;
use ratatui::{crossterm::event::KeyEvent, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, widgets::{Block, Borders, WidgetRef}, Frame};
use crate::terminal_mode::{events::action_handler::WidgetEvent, fx::{effect::UniqueEffectId, EffectStage}, widgets::service_form::ServiceFormWidget};
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};
use crate::terminal_mode::widgets::HandleWidget;
use ratatui::crossterm::event::MouseEvent;
use ratatui::prelude::*;
use std::cell::RefCell;

// Define a virtual height for the service form content.
const SERVICE_FORM_VIRTUAL_HEIGHT: u16 = 50; // adjust as needed

////////////////////////////////
// TUR SHEET TAB with SERVICE NUM INPUT
////////////////////////////////
/// ServiceTab Component
pub struct ServiceTab<'a> {
    // Wrap the widget so it can be shared.
    pub service_form_widget: std::rc::Rc<std::cell::RefCell<ServiceFormWidget<'a>>>,
    scroll_state: RefCell<ScrollViewState>,
    pub effect_stage: EffectStage<UniqueEffectId>,
    last_service_form_area: RefCell<Option<Rect>>,

}

impl<'a> ServiceTab<'a> {
    pub fn new(event_sender: Sender<WidgetEvent>) -> Self {
        let service_form_widget = std::rc::Rc::new(std::cell::RefCell::new(
            ServiceFormWidget::new(event_sender)
        ));
        Self {
            service_form_widget: service_form_widget,
            scroll_state: RefCell::new(ScrollViewState::default()),
            effect_stage: EffectStage::default(),
            last_service_form_area: RefCell::new(None)
        }
    }
}

impl <'a> HandleWidget <'_> for ServiceTab <'_> {
    /// Draw the entire ServiceTab, including its buttons
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        // ----- Process TachyonFX Effects -----
        // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
        let fx_duration = tachyonfx::Duration::from_millis(16);
        // Process all effects added to our effect_stage. They will update and render onto f's buffer.
        self.effect_stage.process_effects(fx_duration, f.buffer_mut(), area);

        // Divide the area into vertical chunks (input row + main content)
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(area);
    
        let area_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(5),
                Constraint::Percentage(95),
            ])
            .split(vertical_chunks[1]);

        let json_view_title_area = area_chunks[0];
        let service_form_area = area_chunks[1];

        // Save the computed visible area for later event handling.
        self.last_service_form_area.replace(Some(service_form_area));

        let service_form_title = Block::new()
            .title(Line::from("Service Form").centered())
            .borders(Borders::BOTTOM)
            .border_type(ratatui::widgets::BorderType::Rounded);

        // JSON viewer
        let mut borders = Borders::RIGHT;
        borders.set(Borders::LEFT, true);

        // Create a scroll view with a fixed virtual content size.
        // This ensures that even if `service_form_area` (the visible area) is small,
        // the service form widget is rendered into a larger virtual buffer.
        let virtual_size = Size {
            width: service_form_area.width,
            height: SERVICE_FORM_VIRTUAL_HEIGHT,
        };
        let mut scroll_view = ScrollView::new(virtual_size)
            .vertical_scrollbar_visibility(
                ScrollbarVisibility::Automatic
            )
            .horizontal_scrollbar_visibility(ScrollbarVisibility::Never);

        let svc_form = self.service_form_widget.borrow();
        svc_form.render_ref(scroll_view.area(), scroll_view.buf_mut());

        // Render JSON viewer scroll view.
        service_form_title.render_ref(json_view_title_area, f.buffer_mut());
        scroll_view.render(service_form_area, f.buffer_mut(), &mut self.scroll_state.borrow_mut());
    }
    
    /// Handle a mouse event, see if it hits our get_ticket_button or submit_button
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        match mouse_event.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollDown => self.scroll_state.borrow_mut().scroll_down(),
            ratatui::crossterm::event::MouseEventKind::ScrollUp => self.scroll_state.borrow_mut().scroll_up(),
            ratatui::crossterm::event::MouseEventKind::ScrollLeft => self.scroll_state.borrow_mut().scroll_left(),
            ratatui::crossterm::event::MouseEventKind::ScrollRight => self.scroll_state.borrow_mut().scroll_right(),
            _ => {
                self.service_form_widget.borrow().check_active_field();
                // let service_num_is_active = self.order_number_field.is_active();

                // Now, forward the event to the ServiceFormWidget.
                // self.service_form_widget.handle_mouse_event(&mouse_event);
                // self.service_form_widget.update_focus_from_mouse(&mouse_event);
                // Retrieve the visible area for the service form from our stored state.
                if let Some(visible_area) = *self.last_service_form_area.borrow() {
                    let scroll_offset = self.scroll_state.borrow().offset().y; 
                    let local_y = mouse_event.row
                        .saturating_sub(visible_area.y)
                        .saturating_add(scroll_offset);
                    
                    let local_x = mouse_event.column
                        .saturating_sub(visible_area.x);

                    // Convert the global mouse event coordinates into local coordinates relative to visible_area.
                    let local_mouse_event = MouseEvent {
                        column: local_x,
                        row: local_y,
                        kind: mouse_event.kind,
                        modifiers: mouse_event.modifiers,
                    };
                    let c = mouse_event.column;
                    let r = mouse_event.row;
                    let inside = c >= visible_area.x 
                        && c < visible_area.x + visible_area.width 
                        && r >= visible_area.y 
                        && r < visible_area.y + visible_area.height;
                    if inside {
                        // if service_num_is_active && mouse_event.kind == MouseEventKind::Down(crossterm::event::MouseButton::Right) {
                        //     self.order_number_field.set_state(crate::terminal_mode::widgets::button::State::Normal);
                        // }
                        // Now forward the local event to the service form widget.
                        // self.service_form_widget.update_focus_from_mouse(&local_mouse_event);
                        self.service_form_widget.borrow_mut().handle_mouse_event(&local_mouse_event);
                    } else {
                        let is_active = *self.service_form_widget.borrow().active_field.borrow();
                        if is_active.is_some() {
                            // self.service_form_widget.reset_all_states();
                        }
                    }
                }
                
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        self.service_form_widget.borrow_mut().handle_key_event(key_event)
    }
}