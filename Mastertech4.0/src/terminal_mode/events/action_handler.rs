use database::schema::prestashop_schema::PrestashopPayload;
use crossbeam::channel::{Receiver, Sender};
use once_cell::sync::Lazy;

// Define a global event sender (wrapped in `Arc<Mutex<T>>` for safe access)
static GLOBAL_EVENT_SENDER: Lazy<(Sender<WidgetEvent>, Receiver<WidgetEvent>)> = Lazy::new(|| crossbeam::channel::unbounded());

pub fn get_event_sender() -> Sender<WidgetEvent> {
    GLOBAL_EVENT_SENDER.0.clone()
}

pub fn get_event_receiver() -> Receiver<WidgetEvent> {
    GLOBAL_EVENT_SENDER.1.clone()
}


/// An enum representing all widget identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WidgetId(pub String);

#[derive(Debug, Clone, PartialEq)]
pub enum ApiEvent {
    GetTicketResponse(PrestashopPayload)
}

/// A common event enum that all widgets use.
pub enum WidgetEvent {
    ButtonClick { widget_id: WidgetId},
    // UpdateText { widget_id: WidgetId, text: String },
    Api(ApiEvent)
}

/// Trait for any widget (or component) that can handle events.
pub trait ActionHandler {
    // Returns the unique widget identifier.
    // fn widget_id(&self) -> WidgetId;
    /// Process an incoming event.
    fn handle_event(&mut self, event: &WidgetEvent);
}

/// A centralized event manager that receives events from a global channel
/// and dispatches them to registered ActionHandlers.
pub struct EventManager <'a>{
    receiver: crossbeam::channel::Receiver<WidgetEvent>,
    // Instead of owning Box<dyn ActionHandler>, we use shared ownership.
    handlers: Vec<std::rc::Rc<std::cell::RefCell<dyn ActionHandler + 'a>>>,
}

impl <'a> EventManager <'a> {
    pub fn new(receiver: crossbeam::channel::Receiver<WidgetEvent>) -> Self {
        Self {
            receiver,
            handlers: Vec::new(),
        }
    }

    pub fn register_handler(&mut self, handler: std::rc::Rc<std::cell::RefCell<dyn ActionHandler + 'a>>) {
        self.handlers.push(handler);
    }

    pub fn process_events(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            for handler in self.handlers.iter() {
                // Borrow mutably to dispatch the event.
                handler.borrow_mut().handle_event(&event);
            }
        }
    }
}