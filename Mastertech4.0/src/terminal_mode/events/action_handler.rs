use database::schema::{prestashop_schema::PrestashopPayload, CarboniteResponse};
use crossbeam::channel::{Receiver, Sender};
use std::collections::HashMap;
use once_cell::sync::Lazy;

use crate::filesystem::get_client_hash;

// Define a global event sender (wrapped in `Arc<Mutex<T>>` for safe access)
static GLOBAL_EVENT_SENDER: Lazy<(Sender<WidgetEvent>, Receiver<WidgetEvent>)> = Lazy::new(|| crossbeam::channel::unbounded());

// Define global update channel for widget ID updates
static UPDATE_CHANNEL: Lazy<(Sender<WidgetId>, Receiver<WidgetId>)> = Lazy::new(|| crossbeam::channel::unbounded());

pub fn get_event_sender() -> Sender<WidgetEvent> {
    GLOBAL_EVENT_SENDER.0.clone()
}

pub fn get_event_receiver() -> Receiver<WidgetEvent> {
    GLOBAL_EVENT_SENDER.1.clone()
}

pub fn get_update_sender() -> Sender<WidgetId> {
    UPDATE_CHANNEL.0.clone()
}

/// An enum representing all widget identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WidgetId(pub String);

#[derive(Debug, Clone, PartialEq)]
pub enum ApiEvent {
    GetTicketResponse(PrestashopPayload),
    GetSebResponse(Vec<CarboniteResponse>)
}

#[derive(Debug, Clone, PartialEq)]
pub enum WidgetButton {
    Left,
    Right
}
// pub enum WhoAmI {
//     Source(String),
// }

/// A common event enum that all widgets use.
pub enum WidgetEvent {
    ButtonClick { widget_id: WidgetId, button: WidgetButton, source: String },
    Active { widget_id: WidgetId },
    // UpdateText { widget_id: WidgetId, text: String },
    Api(ApiEvent)
}

impl WidgetEvent {
    pub fn is_source_me(&self) -> bool {
        if let Self::ButtonClick { source, ..} = self {
            if *source == get_client_hash().connection_string {
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

/// Trait for any widget (or component) that can handle events.
pub trait ActionHandler {
    // Returns the unique widget identifier.
    fn widget_id(&self) -> WidgetId;
    /// Process an incoming event.
    fn handle_event(&mut self, event: &WidgetEvent);

    fn managed_widget_ids(&self) -> Vec<WidgetId>;
}

/// A centralized event manager that receives events from a global channel
/// and dispatches them to registered ActionHandlers.
pub struct EventManager <'a>{
    receiver: crossbeam::channel::Receiver<WidgetEvent>,
    update_receiver: crossbeam::channel::Receiver<WidgetId>,
    // Instead of owning Box<dyn ActionHandler>, we use shared ownership.
    handlers: Vec<(WidgetId, std::rc::Rc<std::cell::RefCell<dyn ActionHandler + 'a>>)>,
    widget_to_handler: HashMap<WidgetId, std::rc::Rc<std::cell::RefCell<dyn ActionHandler + 'a>>>,
}

impl <'a> EventManager <'a> {
    pub fn new(receiver: crossbeam::channel::Receiver<WidgetEvent>) -> Self {
        Self {
            receiver,
            update_receiver: UPDATE_CHANNEL.1.clone(),
            handlers: Vec::new(),
            widget_to_handler: HashMap::new(),
        }
    }

    pub fn register_handler(&mut self, handler: std::rc::Rc<std::cell::RefCell<dyn ActionHandler + 'a>>) {
        let handler_id = handler.borrow().widget_id();
        let widget_ids = handler.borrow().managed_widget_ids();
        for widget_id in &widget_ids {
            if let Some(existing) = self.widget_to_handler.get(widget_id) {
                log::warn!(
                    "Widget ID {} is already registered to handler {:?}, overwriting with {:?}",
                    widget_id.0,
                    existing.borrow().widget_id(),
                    handler_id
                );
            }
            self.widget_to_handler.insert(widget_id.clone(), handler.clone());
        }
        self.handlers.push((handler_id, handler));
    }

    pub fn update_handler(&mut self, handler_id: WidgetId) {
        if let Some((_, handler)) = self.handlers.iter().find(|(id, _)| *id == handler_id) {
            let handler_ref = handler.borrow();
            let widget_ids = handler_ref.managed_widget_ids();
            drop(handler_ref);

            // Remove old mappings for this handler
            self.widget_to_handler.retain(|_, h| {
                let h_ref = h.borrow();
                !std::rc::Rc::ptr_eq(h, handler) || h_ref.widget_id() != handler_id
            });

            // Add new mappings
            for widget_id in widget_ids {
                if let Some(existing) = self.widget_to_handler.insert(widget_id.clone(), handler.clone()) {
                    log::warn!(
                        "Widget ID {} reassigned from handler {} to {}",
                        widget_id.0,
                        existing.borrow().widget_id().0,
                        handler_id.0
                    );
                }
            }
        }
    }

    pub fn process_events(&mut self) {
        // Handle widget ID updates
        while let Ok(handler_id) = self.update_receiver.try_recv() {
            log::debug!("Received update request for handler: {}", handler_id.0);
            self.update_handler(handler_id);
        }

        while let Ok(event) = self.receiver.try_recv() {
            let event_widget_id = match &event {
                WidgetEvent::ButtonClick { widget_id, .. } => Some(widget_id),
                WidgetEvent::Active { widget_id } => Some(widget_id),
                WidgetEvent::Api(_) => None,
            };

            if let Some(widget_id) = event_widget_id {
                if let Some(handler) = self.widget_to_handler.get(widget_id) {
                    let mut handler_mut = handler.borrow_mut();
                    if self.handlers.iter().any(|(id, h)| 
                        *id == handler_mut.widget_id() && std::rc::Rc::ptr_eq(h, handler)
                    ) {
                        handler_mut.handle_event(&event);
                    }
                }
            } else {
                // Broadcast untargeted events (e.g., Api events) to all handlers
                for (_, handler) in self.handlers.iter() {
                    handler.borrow_mut().handle_event(&event);
                }
            }
        }
    }
}