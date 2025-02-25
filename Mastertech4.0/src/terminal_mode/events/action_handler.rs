use crate::terminal_mode::{data::ServiceData, tabs::ServiceTab, widgets::service_form::ServiceFormWidget};

pub enum WidgetEvent {
    ButtonClick,
    UpdateText(String),
    
}


pub trait ActionHandler<T> {
    fn handle_action(&self, arg: T);
}


// impl <'a>ActionHandler<&'a str> for ServiceTab<'a> {
//     fn handle_action(&self, arg: &'a str) {
//         self.get_ticket(arg);
//     }
// }

impl <'a>ActionHandler<ServiceData> for ServiceFormWidget<'a> {
    fn handle_action(&self, arg: ServiceData) {

        
    }
}